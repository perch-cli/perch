//! A Host that keeps the world in memory and records what it was asked to do.
//!
//! Behavior tests drive real command code against this and assert on observable
//! outcomes: what was printed, what ended up in the keychain, and what went out
//! to the network. A machine with no arranged replies has no network at all, so
//! a command that fetches when it should not fails here rather than quietly
//! passing (ADR a-figure-carries-its-age).
//!
//! The world is held in the port's own nine concerns, and the port is named
//! `port::` here because all nine of its trait names are also the state's.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};

// `impl port::Files for FakeHost` says *this is the surface*; the bare
// `Filesystem` says *this is the world* (ADR the-port-fits-the-machine).
use super as port;
use super::{
    Execution, HostError, HttpRequest, HttpResponse, Link, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE,
    Platform, Waited,
};
// The methods, without the names: a trait has to be in scope to be found even
// where its name is not wanted. Through `port` rather than `super`, because two
// of these three names are also the state's.
use crate::keychain::KeychainError;
use port::{Environment as _, Files as _, Processes as _};
use zeroize::Zeroizing;

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
    /// One path made to stand for another, and how.
    Linked {
        kind: Link,
        target: PathBuf,
        at: PathBuf,
    },
    RemovedLink(PathBuf),
    Touched(PathBuf),
    Slept {
        millis: u64,
    },
    /// A wait a loop can be interrupted out of — the watcher's poll interval,
    /// and nothing else. Distinct from a sleep because they answer different
    /// questions: how long Perch waited for somebody else's lock, and how many
    /// times round the loop went.
    Waited {
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
        args: Vec<String>,
        config_dir: PathBuf,
        /// What was added to the launched program's environment. Kept whole
        /// rather than only as `config_dir`, because it is how an Upgrade tells
        /// the embedded installer which Release to fetch
        /// (ADR an-upgrade-asks-its-channel).
        env: Vec<(String, String)>,
    },
    /// A request that went out to the network.
    Http {
        url: String,
    },
    /// A question put to the person at the terminal. Recorded so a command that
    /// must never ask one — bare `perch switch` (ADR perch-does-not-draw) — can
    /// be held to it.
    Asked,
    /// A question whose answer the terminal never showed. Distinct from `Asked`
    /// so a test can say that a passphrase went in by the path that hides it
    /// rather than by the one that echoes.
    AskedInSecret,
}

/// One request the fake was asked to send, kept whole so a test can say what
/// went out as well as where it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    /// The bound the caller put on it, if any. Kept because a request that was
    /// supposed to be abandoned quickly and was not is invisible in a test that
    /// only looks at where it went.
    pub within_millis: Option<u64>,
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
/// A directory that will hold a Credential is not this.
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

/// Something that happens while Perch waits — contending for a lock, or putting
/// a question to the person at the terminal. A `claude` started during a lock
/// wait is what the questions asked *under* the locks are asked again for, and
/// another `perch` claiming an abandoned lock while somebody stares at a `[y/N]`
/// is what the hold is re-checked for.
pub type WhileWaiting = Box<dyn Fn(&FakeHost)>;

/// The replies an endpoint gives one asker, in the order it gives them: keyed
/// the way a single reply is, by URL and by the access token that asked.
type Traces = BTreeMap<(String, Option<String>), VecDeque<HttpResponse>>;

/// What a path is refusing to do, for the fixture that says so.
///
/// `List` is a directory there and unwalkable that still answers when it was last
/// written — the state a lock left root-owned inside a Profile is in, and the one
/// `lock::clear_the_abandoned` has to tell from a plain file wedging the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusing {
    Read,
    List,
    Write,
    Delete,
}

/// The world, held in the pieces the port names it in. *State and nothing else*
/// — every method stays on `FakeHost`, so one that has to touch the clock writes
/// `self.stall.now` and `self.fs.modified` in the same breath. The `RefCell`
/// stays per field, because the fake is deliberately re-entered mid-call by a
/// `somebody_else` closure and eight struct-wide borrows would panic.
pub struct FakeHost {
    environment: Environment,
    fs: Filesystem,
    keys: Keys,
    processes: Processes,
    waiting: Waiting,
    terminal: Terminal,
    network: Network,
    stall: Stall,
    /// What the fake was asked to do, in order. Written from seven of the nine
    /// concerns and read back whole, so it is cross-cutting on purpose and stays
    /// one bare field.
    effects: RefCell<Vec<Effect>>,
}

/// The machine Perch was started on: where it runs, as whom, and as what.
struct Environment {
    home: PathBuf,
    /// The directory the command was typed in — the project a Run is about, and
    /// the one entry of `projects` that crosses to a Profile
    /// (ADR everything-but-the-account).
    current_dir: RefCell<PathBuf>,
    platform: RefCell<Platform>,
    vars: RefCell<BTreeMap<String, String>>,
    /// Where this Perch's own binary sits. Nowhere any Channel would have put
    /// it, so a test about which Channel installed the machine has to say — and
    /// one that is not about it reads as an Installation Perch did not make.
    current_exe: RefCell<PathBuf>,
    /// Which user this Perch runs as. `Some(0)` is the one `watcher install`
    /// refuses; the default is an ordinary person's.
    user_id: RefCell<Option<u32>>,
}

/// What is on disk, links included. Fourteen fields and not split into `Files`
/// and `Links`, because the traffic runs both ways: `files`, `modes`, `dirs`,
/// `modified`, `unwritable` and `undeletable` are all read from the
/// `port::Links` block, and `links` is read from the `port::Files` one.
struct Filesystem {
    files: RefCell<BTreeMap<PathBuf, String>>,
    /// The permissions of everything that has any, so a test can say that a
    /// Credential was never briefly readable by anyone else.
    modes: RefCell<BTreeMap<PathBuf, u32>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    modified: RefCell<BTreeMap<PathBuf, DateTime<Utc>>>,
    unreadable: RefCell<BTreeMap<PathBuf, String>>,
    /// Paths that say when they were written and will not hand over their
    /// contents. A file's own mode refuses the `open` while its directory
    /// answers the `stat`, so `unreadable` — which refuses both — cannot stand
    /// in for the root-owned file a `sudo claude` leaves.
    unopenable: RefCell<BTreeMap<PathBuf, String>>,
    /// Paths whose bytes are not text, which the fake cannot hold as one.
    not_text: RefCell<BTreeSet<PathBuf>>,
    unwritable: RefCell<BTreeMap<PathBuf, String>>,
    /// Paths that take so many more writes and then refuse every one after
    /// them. What `unwritable` cannot stand in for: a command whose first write
    /// has to land and whose third must not has no closure a test could reach
    /// between the two.
    unwritable_after: RefCell<BTreeMap<PathBuf, (usize, String)>>,
    /// Paths that will not give up what they hold. Distinct from an unwritable
    /// one: a store that refuses a write is routinely one a superseded copy can
    /// still be cleared out of.
    undeletable: RefCell<BTreeMap<PathBuf, String>>,
    /// Directories that are there and will not be walked, while still answering
    /// everything asked about them from the outside. Distinct from `unreadable`,
    /// which stops `modified_at` too: a directory whose read bit is gone is
    /// stat-ed from its parent perfectly well, and
    /// `lock::clear_the_abandoned` turns on that difference.
    unlistable: RefCell<BTreeMap<PathBuf, String>>,
    /// Files that come back different from how they were written.
    corrupting: RefCell<BTreeSet<PathBuf>>,
    /// Files whose write dies partway, leaving what fitted behind.
    filling: RefCell<BTreeSet<PathBuf>>,
    /// Every link that has been made, by the path that holds it. A hard link is
    /// in here *and* in `files`, because that is what a hard link is: another
    /// name for the same bytes, telling nothing about itself.
    links: RefCell<BTreeMap<PathBuf, (Link, PathBuf)>>,
    /// Whether this Windows can make a symbolic link. Off is the ordinary
    /// machine: the privilege needs Developer Mode or elevation, which is why a
    /// Run reaches for junctions and hard links.
    developer_mode: RefCell<bool>,
}

/// The Credential Store that is a keychain, and the ways it refuses.
struct Keys {
    keychain: RefCell<BTreeMap<(String, String), String>>,
    keychain_lock: RefCell<Option<KeychainLock>>,
    /// Whether this machine has a keychain although it is not a Mac.
    keychain_everywhere: RefCell<bool>,
    /// How many bytes of a Credential the keychain keeps, when a test is about
    /// a store that takes a write and quietly stores less than it was given.
    keychain_keeps: RefCell<Option<usize>>,
    /// How long a keychain write takes to come back — a permission dialog
    /// somebody has to answer, which is the one step in a Switch that can stall
    /// for minutes without warning. Here rather than in [`Stall`] because it
    /// says how long *this* surface stalls, and one concern writes and reads
    /// it.
    keychain_set_takes_millis: RefCell<u64>,
}

/// What other programs are, and what happens when Perch runs one.
struct Processes {
    executions: RefCell<BTreeMap<String, Execution>>,
    login: RefCell<Option<Login>>,
    /// The running processes, each with when it began — or `None` for one whose
    /// start the operating system will not say.
    live_processes: RefCell<BTreeMap<u32, Option<DateTime<Utc>>>>,
}

/// A loop that goes round until somebody stops it.
struct Waiting {
    /// Whether anything is listening for the person at the terminal asking a
    /// loop to stop.
    listening: RefCell<bool>,
    /// How many waits go by before that is asked for. `None` is a machine
    /// nobody interrupts, which is every test but the watcher's.
    interrupt_after: RefCell<Option<u32>>,
    /// The same, counted in requests that have gone out — for a stop that lands
    /// inside a round rather than in the wait between two.
    interrupt_after_requests: RefCell<Option<usize>>,
    waits: RefCell<u32>,
}

/// The person Perch is talking to, and what they say back.
struct Terminal {
    notes: RefCell<Vec<String>>,
    interactive: RefCell<bool>,
    answers: RefCell<VecDeque<String>>,
    /// What the person types where the terminal shows nothing back. Its own
    /// queue rather than a share of `answers`, so a test states which prompt
    /// each line was typed at — and a command that read a passphrase through the
    /// echoing path runs out of answers rather than quietly working.
    secrets: RefCell<VecDeque<String>>,
    /// How long the person at the terminal takes over each answer. Here rather
    /// than in [`Stall`] for the same reason `keychain_set_takes_millis` is in
    /// [`Keys`]: it is how long one surface stalls.
    answering_takes_millis: RefCell<u64>,
}

/// What is out there to be asked, and what went out to ask it.
struct Network {
    /// What each endpoint answers, by URL and by the access token that asked.
    replies: RefCell<BTreeMap<(String, Option<String>), HttpResponse>>,
    /// What an endpoint answers each time it is asked, where a test is about a
    /// figure that moves: the trace an Account's Utilization follows while the
    /// watcher watches it.
    traces: RefCell<Traces>,
    sent: RefCell<Vec<Sent>>,
    /// How long one request takes to answer. A burst of reads is bounded by
    /// nothing but the network, so a round can outlast a hold renewed either
    /// side of it.
    takes_millis: RefCell<u64>,
}

/// Time, and what somebody else does while it passes: one mechanism with two
/// fields rather than a clock beside a hook. Time does not pass in this fake
/// because a clock ticks; it passes because an effect took time, and while that
/// effect was in flight somebody else touched the machine. So there is no
/// `Clock` struct — `impl port::Clock for FakeHost` reads `self.stall.now`.
struct Stall {
    now: RefCell<DateTime<Utc>>,
    /// What the rest of the machine does the first time Perch waits, and then
    /// does not do again.
    somebody_else: RefCell<Option<WhileWaiting>>,
    /// The same, once this many requests have gone out. Counted in requests
    /// rather than in waits because a burst takes no wait at all: what happens
    /// between two of its reads has nowhere else to be hung.
    somebody_else_after_requests: RefCell<Option<(usize, WhileWaiting)>>,
}

/// The pid this Perch runs under, for the tests that assert on the marker a Run
/// writes for itself. Nothing else in the fixtures wears it.
pub const THIS_PROCESS: u32 = 700;

/// The uid this Perch runs as. An ordinary person's, deliberately: `0` is the
/// one value `perch watcher install` refuses, so it must never be what a test
/// gets without asking for it.
pub const THIS_USER: u32 = 501;

impl Default for FakeHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Arranging a world, and reading back what Perch did to it. The half of this
/// file that grows fastest: a Host method arrives with the builders a test needs
/// to set it up, so `process_started_at` costs ten lines of trait, thirty-six
/// here, and four `with_*` methods for one concept. One file with the `impl`
/// blocks below, because eleven of the last twenty-five commits touched both.
impl FakeHost {
    pub fn new() -> Self {
        let mut vars = BTreeMap::new();
        vars.insert("USER".to_string(), "someone".to_string());
        FakeHost {
            environment: Environment {
                home: PathBuf::from("/Users/someone"),
                // Spelled out rather than joined onto `home`: `join` answers
                // with the separator of whatever platform the tests run on, and
                // the two spellings would stop matching on Windows.
                current_dir: RefCell::new(PathBuf::from("/Users/someone/work")),
                // The platform Perch was written for first, so a test says
                // which Credential Store it is about only when that is what it
                // is about.
                platform: RefCell::new(Platform::MacOs),
                vars: RefCell::new(vars),
                current_exe: RefCell::new(PathBuf::from("/somewhere/nobody/installs/perch")),
                user_id: RefCell::new(Some(THIS_USER)),
            },
            fs: Filesystem {
                files: RefCell::new(BTreeMap::new()),
                modes: RefCell::new(BTreeMap::new()),
                dirs: RefCell::new(BTreeSet::new()),
                modified: RefCell::new(BTreeMap::new()),
                unreadable: RefCell::new(BTreeMap::new()),
                unopenable: RefCell::new(BTreeMap::new()),
                not_text: RefCell::new(BTreeSet::new()),
                unwritable: RefCell::new(BTreeMap::new()),
                unwritable_after: RefCell::new(BTreeMap::new()),
                unlistable: RefCell::new(BTreeMap::new()),
                undeletable: RefCell::new(BTreeMap::new()),
                corrupting: RefCell::new(BTreeSet::new()),
                filling: RefCell::new(BTreeSet::new()),
                links: RefCell::new(BTreeMap::new()),
                developer_mode: RefCell::new(false),
            },
            keys: Keys {
                keychain: RefCell::new(BTreeMap::new()),
                keychain_lock: RefCell::new(None),
                keychain_everywhere: RefCell::new(false),
                keychain_keeps: RefCell::new(None),
                keychain_set_takes_millis: RefCell::new(0),
            },
            processes: Processes {
                executions: RefCell::new(BTreeMap::new()),
                login: RefCell::new(None),
                // This Perch, running since before any session a fixture
                // records, so a marker a Run writes for itself corroborates the
                // way a real one does. Nothing like the fixtures' other pids.
                live_processes: RefCell::new(BTreeMap::from([(
                    THIS_PROCESS,
                    Some(DateTime::<Utc>::MIN_UTC),
                )])),
            },
            waiting: Waiting {
                listening: RefCell::new(false),
                interrupt_after: RefCell::new(None),
                interrupt_after_requests: RefCell::new(None),
                waits: RefCell::new(0),
            },
            terminal: Terminal {
                notes: RefCell::new(Vec::new()),
                interactive: RefCell::new(true),
                answers: RefCell::new(VecDeque::new()),
                secrets: RefCell::new(VecDeque::new()),
                answering_takes_millis: RefCell::new(0),
            },
            network: Network {
                replies: RefCell::new(BTreeMap::new()),
                traces: RefCell::new(BTreeMap::new()),
                sent: RefCell::new(Vec::new()),
                takes_millis: RefCell::new(0),
            },
            stall: Stall {
                now: RefCell::new(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()),
                somebody_else: RefCell::new(None),
                somebody_else_after_requests: RefCell::new(None),
            },
            effects: RefCell::new(Vec::new()),
        }
    }

    // A builder sits with the struct it arranges, in declaration order. The
    // names say nothing about which: the grouping is for whoever edits here.

    // ---- environment: the machine it runs on ------------------------

    /// A machine that is not a Mac, where a Credential lives in a file rather
    /// than in a keychain.
    pub fn with_platform(self, platform: Platform) -> Self {
        *self.environment.platform.borrow_mut() = platform;
        self
    }

    /// The directory the command is being typed in — a second repository, for
    /// a test about the project entry that crosses being the current one.
    pub fn in_directory(self, path: impl AsRef<Path>) -> Self {
        *self.environment.current_dir.borrow_mut() = path.as_ref().to_path_buf();
        self
    }

    /// Where this Perch's binary sits, which is the whole of what says which
    /// Channel installed it.
    pub fn installed_at(self, path: impl AsRef<Path>) -> Self {
        *self.environment.current_exe.borrow_mut() = path.as_ref().to_path_buf();
        self
    }

    /// An environment variable the machine has set.
    pub fn with_env(self, key: &str, value: &str) -> Self {
        self.environment
            .vars
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
        self
    }

    /// A variable the machine does not have — `USER` on Windows, most notably.
    pub fn without_env(self, key: &str) -> Self {
        self.environment.vars.borrow_mut().remove(key);
        self
    }

    /// A machine where Perch was run with `sudo`, which is the one thing `perch
    /// watcher install` refuses outright (ADR the-machine-runs-the-watcher).
    pub fn as_superuser(self) -> FakeHost {
        *self.environment.user_id.borrow_mut() = Some(0);
        self
    }

    // ---- fs: what is on disk ----------------------------------------

    pub fn with_file(self, path: impl AsRef<Path>, contents: &str) -> Self {
        self.set_file(path, contents);
        self
    }

    /// The same, for a world being arranged from inside a [`Login`], where the
    /// fake is already borrowed and cannot be consumed.
    pub fn set_file(&self, path: impl AsRef<Path>, contents: &str) {
        self.note_directories_of(path.as_ref());
        self.fs
            .files
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), contents.to_string());
        // A file on a real filesystem has an age. Without this `modified_at`
        // answers `NotFound` where `path_exists` says there is a file, and
        // `carry`'s ranking by age sees every candidate as never used.
        self.mark_written(path.as_ref());
    }

    /// A file exists in the directories above it, so they exist too. Without
    /// this a directory could hold something and still not be there to list.
    fn note_directories_of(&self, path: &Path) {
        let mut dirs = self.fs.dirs.borrow_mut();
        let mut at = path.parent();
        while let Some(dir) = at {
            if dir.as_os_str().is_empty() {
                break;
            }
            dirs.insert(dir.to_path_buf());
            at = dir.parent();
        }
    }

    /// A file somebody else could read: one written by an older Claude Code,
    /// or restored from a backup that forgot its mode.
    pub fn with_file_mode(self, path: impl AsRef<Path>, mode: u32) -> Self {
        self.fs
            .modes
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), mode);
        self
    }

    /// A file whose bytes are not text — the answer `read_to_string` gives for
    /// anything binary, which is what a plain `age -p` file is.
    ///
    /// Held as a flag rather than as contents because the fake keeps files as
    /// `String`, so the state cannot be spelled any other way.
    pub fn with_a_file_that_is_not_text(self, path: impl AsRef<Path>) -> Self {
        self.fs
            .files
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), String::new());
        self.fs
            .not_text
            .borrow_mut()
            .insert(path.as_ref().to_path_buf());
        self
    }

    /// A file that is there but cannot be read — the wrong permissions, most
    /// often. Distinct from a file that is simply absent.
    /// A file that says when it was written and will not be opened.
    pub fn with_a_file_that_will_not_open(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.fs
            .unopenable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// A path refusing one of the four things a path can refuse, from the start.
    ///
    /// One axis rather than a method per kind: the three moments a test needs — so
    /// from the start, so partway through, and put right again — are the same three
    /// whichever refusal it is.
    pub fn with_a_path_refusing(
        self,
        path: impl AsRef<Path>,
        refusing: Refusing,
        detail: &str,
    ) -> Self {
        self.now_refusing(path, refusing, detail);
        self
    }

    /// The same, for a test whose subject is a path that starts refusing *while* a
    /// command is running — a lock artifact somebody changes the permissions of
    /// mid-hold — rather than one that refused from the start.
    pub fn now_refusing(&self, path: impl AsRef<Path>, refusing: Refusing, detail: &str) {
        self.refusals(refusing)
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
    }

    /// Whatever the path was refusing over has been put right — the permission fixed,
    /// the disk freed, the holder let go — so a test can carry on from a failure with
    /// the world it left rather than a fresh one.
    pub fn no_longer_refusing(&self, path: impl AsRef<Path>, refusing: Refusing) {
        self.refusals(refusing).borrow_mut().remove(path.as_ref());
    }

    fn refusals(&self, refusing: Refusing) -> &RefCell<BTreeMap<PathBuf, String>> {
        match refusing {
            Refusing::Read => &self.fs.unreadable,
            Refusing::List => &self.fs.unlistable,
            Refusing::Write => &self.fs.unwritable,
            Refusing::Delete => &self.fs.undeletable,
        }
    }

    /// A path that takes `writes` more writes and refuses every one after them.
    /// The failure a command meets partway through its own write sequence — a
    /// lock taken over, a volume remounted read-only — where
    /// [`set_unwritable`](Self::set_unwritable) has no moment to be called at
    /// because the command hands the test no closure between the two writes.
    pub fn with_a_file_unwritable_after(
        self,
        path: impl AsRef<Path>,
        writes: usize,
        detail: &str,
    ) -> Self {
        self.fs
            .unwritable_after
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), (writes, detail.to_string()));
        self
    }
    /// A file that comes back different from how it was written — the same
    /// hazard on the plaintext store, which the same guard covers.
    pub fn with_file_corrupting_writes(self, path: impl AsRef<Path>) -> Self {
        self.fs
            .corrupting
            .borrow_mut()
            .insert(path.as_ref().to_path_buf());
        self
    }

    /// A disk that fills partway through the write rather than before it: the
    /// bytes that fitted are on disk, and the call fails. Distinct from
    /// [`with_unwritable_file`](Self::with_unwritable_file), which is the write
    /// never starting — and it is what reaches `replace_via_tmp`'s cleanup,
    /// which a fake that could only fail before creating anything cannot.
    pub fn with_a_disk_that_fills_writing(self, path: impl AsRef<Path>) -> Self {
        self.fs
            .filling
            .borrow_mut()
            .insert(path.as_ref().to_path_buf());
        self
    }

    /// A directory somebody else already holds, last written when they say.
    /// A lock artifact, in other words: the age is what decides whether the
    /// holder is taken to be alive or to have died holding it.
    pub fn with_dir_held_since(self, path: impl AsRef<Path>, since: DateTime<Utc>) -> Self {
        // The directories above it too, as `with_link` and `with_file` do: a
        // lock inside a Profile that does not exist is a world no machine can
        // produce, since `create_dir_exclusive` is `mkdir`.
        self.note_directories_of(path.as_ref());
        self.fs
            .dirs
            .borrow_mut()
            .insert(path.as_ref().to_path_buf());
        self.fs
            .modified
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), since);
        self
    }

    /// A link that is already there — including one pointing at nothing, which is
    /// what a Profile holds after the entry it shared was deleted, and the state
    /// a Reconcile has to repair.
    pub fn with_link(self, kind: Link, target: impl AsRef<Path>, at: impl AsRef<Path>) -> Self {
        self.note_directories_of(at.as_ref());
        self.fs.links.borrow_mut().insert(
            at.as_ref().to_path_buf(),
            (kind, target.as_ref().to_path_buf()),
        );
        self
    }

    /// A Windows with Developer Mode on, where a symbolic link is something an
    /// ordinary user may make. Means nothing anywhere else, where they always
    /// could.
    pub fn with_developer_mode(self) -> Self {
        *self.fs.developer_mode.borrow_mut() = true;
        self
    }

    // ---- keys: the keychain, and the ways it refuses ----------------

    pub fn with_keychain_item(self, service: &str, account: &str, secret: &str) -> Self {
        self.set_keychain_item(service, account, secret);
        self
    }

    pub fn set_keychain_item(&self, service: &str, account: &str, secret: &str) {
        self.keys.keychain.borrow_mut().insert(
            (service.to_string(), account.to_string()),
            secret.to_string(),
        );
    }

    /// An item that is gone from the keychain — an Account whose Credential
    /// nothing Perch holds can recover.
    pub fn forget_keychain_item(&self, service: &str, account: &str) {
        self.keys
            .keychain
            .borrow_mut()
            .remove(&(service.to_string(), account.to_string()));
    }

    /// A machine that has a keychain although it is not a Mac. Off macOS the
    /// default is a keychain that fails the way a real one would, since
    /// `/usr/bin/security` is not there — so asking for one is a statement that
    /// the test is about the composite reader rather than about that
    /// platform.
    pub fn with_keychain_off_macos(self) -> Self {
        *self.keys.keychain_everywhere.borrow_mut() = true;
        self
    }

    /// Every keychain operation now fails as locked or denied, rather than as
    /// "not found" — the distinction ADR claude-code-chooses-the-store insists
    /// on.
    pub fn with_locked_keychain(self, detail: &str) -> Self {
        self.lock_keychain(detail);
        self
    }

    /// The same, for a world that is already built.
    pub fn lock_keychain(&self, detail: &str) {
        *self.keys.keychain_lock.borrow_mut() = Some(KeychainLock {
            detail: detail.to_string(),
        });
    }

    /// A keychain that has come back — somebody typed their password, or the
    /// screen unlocked. What a locked keychain does to Perch is only half the
    /// story; the other half is what the next command makes of what it left.
    pub fn unlock_keychain(&self) {
        *self.keys.keychain_lock.borrow_mut() = None;
    }

    /// A keychain that takes a write, reports success, and keeps only the first
    /// `bytes` of what it was given — what `security -i` does when a command line
    /// overruns its stdin buffer. Without a store that can do it, the read-back
    /// guard every Credential goes through could be deleted with every test still
    /// passing.
    pub fn with_keychain_truncating_after(self, bytes: usize) -> Self {
        *self.keys.keychain_keeps.borrow_mut() = Some(bytes);
        self
    }

    /// A keychain that stops to ask the user for permission, and how long they
    /// take to answer it — before a read, a write and a delete alike. The stall
    /// a Switch and a Purge both document as unbounded: everything Perch is
    /// holding has to survive somebody walking away from the dialog.
    pub fn with_a_keychain_that_asks_first(self, takes_millis: u64) -> Self {
        *self.keys.keychain_set_takes_millis.borrow_mut() = takes_millis;
        self
    }

    // ---- processes: other programs, running or run ------------------

    pub fn with_exec(self, program: &str, args: &[&str], execution: Execution) -> Self {
        self.set_exec(program, args, execution);
        self
    }

    /// The same, for a world being arranged from inside a [`Login`], where the
    /// fake is already borrowed and cannot be consumed. What a login *changes* is
    /// read back by a later command: `perch relogin` clears a Quarantine, and the
    /// `perch list --json` after it has to say something the one before it did
    /// not.
    pub fn set_exec(&self, program: &str, args: &[&str], execution: Execution) {
        self.processes
            .executions
            .borrow_mut()
            .insert(exec_key(program, args), execution);
    }

    /// What an interactive login leaves behind in the directory it was pointed
    /// at. Without one, a launched login does nothing and exits cleanly — an
    /// abandoned login.
    pub fn with_login(self, login: impl Fn(&FakeHost, &Path) -> i32 + 'static) -> Self {
        *self.processes.login.borrow_mut() = Some(Box::new(login));
        self
    }

    /// A process that is running, and has been since before any session a
    /// fixture records — so a marker naming it means a Live Profile rather
    /// than one a client left behind when it died.
    pub fn with_live_process(self, pid: u32) -> Self {
        self.processes
            .live_processes
            .borrow_mut()
            .insert(pid, Some(DateTime::<Utc>::MIN_UTC));
        self
    }

    /// A process that is running, for a world that is already built.
    pub fn set_live_process(&self, pid: u32) {
        self.processes
            .live_processes
            .borrow_mut()
            .insert(pid, Some(DateTime::<Utc>::MIN_UTC));
    }

    /// A process that is running and began at `at` — a recycled PID is one
    /// wearing a marker that was written before the process began.
    pub fn with_live_process_started_at(self, pid: u32, at: DateTime<Utc>) -> Self {
        self.processes
            .live_processes
            .borrow_mut()
            .insert(pid, Some(at));
        self
    }

    /// A process that is running but whose start the operating system will not
    /// say — the one situation a session marker naming it can be neither
    /// corroborated nor dismissed in.
    pub fn with_live_process_of_unknown_start(self, pid: u32) -> Self {
        self.processes.live_processes.borrow_mut().insert(pid, None);
        self
    }

    /// This Perch, gone: what a Run that was killed rather than exiting leaves
    /// behind, since the marker it wrote outlives the process it names.
    pub fn with_this_process_dead(self) -> Self {
        self.processes
            .live_processes
            .borrow_mut()
            .remove(&self.process_id());
        self
    }

    /// This Perch's pid recycled by a process that began at `at` — a marker
    /// written before that instant names somebody else now
    /// (ADR a-profile-is-live-by-evidence).
    pub fn with_this_process_replaced_at(self, at: DateTime<Utc>) -> Self {
        self.processes
            .live_processes
            .borrow_mut()
            .insert(self.process_id(), Some(at));
        self
    }

    // ---- waiting: a loop, and the person who stops it ---------------

    /// A person who presses Ctrl-C after the loop has waited this many times.
    /// Counted in waits rather than in rounds because that is what the loop asks
    /// the machine: a test says "three polls and then stop", and gets exactly
    /// the three the person watching would have seen.
    pub fn with_interrupt_after(self, waits: u32) -> Self {
        *self.waiting.interrupt_after.borrow_mut() = Some(waits);
        self
    }

    /// The same person, pressing it after this many requests have gone out. A
    /// round is bounded by the network rather than by the clock, so a stop that
    /// lands *inside* one lands between two requests and nowhere else.
    pub fn with_interrupt_after_requests(self, sent: usize) -> Self {
        *self.waiting.interrupt_after_requests.borrow_mut() = Some(sent);
        self
    }

    // ---- terminal: the person Perch is talking to -------------------

    /// A machine with no one at the terminal: over SSH, or in CI.
    pub fn without_terminal(self) -> Self {
        *self.terminal.interactive.borrow_mut() = false;
        self
    }

    /// What the person at the terminal types, in order. Running out of answers
    /// is end of input, not a hang.
    pub fn with_answers(self, answers: &[&str]) -> Self {
        *self.terminal.answers.borrow_mut() = answers.iter().map(|line| line.to_string()).collect();
        self
    }

    /// The same, for the questions the terminal shows nothing back to — a
    /// passphrase, prompted and confirmed (ADR the-holdings-go-out-sealed).
    pub fn with_secrets(self, secrets: &[&str]) -> Self {
        *self.terminal.secrets.borrow_mut() = secrets.iter().map(|line| line.to_string()).collect();
        self
    }

    /// How long the person at the terminal takes to answer each question, which
    /// is instantly by default and never is on a real one. A question put to a
    /// human is the one wait in Perch with no bound on it, and the only place a
    /// command behaving perfectly well can outlast a lock it is holding.
    pub fn with_a_terminal_that_takes(self, millis: u64) -> Self {
        *self.terminal.answering_takes_millis.borrow_mut() = millis;
        self
    }

    // ---- network: what is out there to be asked ---------------------

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

    /// What an endpoint answers each time it is asked, in turn: the trace a
    /// figure follows while something watches it. The last reply answers every
    /// call after it, so a test about a threshold being crossed says the
    /// crossing and not how many rounds the loop has left.
    pub fn with_replies_to(self, url: &str, bearer: &str, replies: &[(u16, &str)]) -> Self {
        self.network.traces.borrow_mut().insert(
            (url.to_string(), Some(bearer.to_string())),
            replies
                .iter()
                .map(|(status, body)| HttpResponse {
                    status: *status,
                    body: Zeroizing::new((*body).to_string()),
                })
                .collect(),
        );
        self
    }

    /// The same, for a world that is already built — a token that only exists
    /// once a Rotation has handed it over, for instance.
    pub fn reply(&self, url: &str, bearer: Option<&str>, status: u16, body: &str) {
        self.network.replies.borrow_mut().insert(
            (url.to_string(), bearer.map(str::to_string)),
            HttpResponse {
                status,
                body: Zeroizing::new(body.to_string()),
            },
        );
    }

    /// A network that answers slowly, and how slowly — per request, as `curl`'s
    /// `max-time` is per request. Longer than the request's bound is a request
    /// that times out rather than one that answers late.
    pub fn with_a_network_that_answers_slowly(self, takes_millis: u64) -> Self {
        *self.network.takes_millis.borrow_mut() = takes_millis;
        self
    }

    // ---- stall: time, and what somebody else does while it passes ---

    pub fn with_now(self, now: DateTime<Utc>) -> Self {
        self.set_now(now);
        self
    }

    /// Moves the clock, for a test where two commands run at different times —
    /// a figure read now and looked at again three minutes later.
    pub fn set_now(&self, now: DateTime<Utc>) {
        *self.stall.now.borrow_mut() = now;
    }

    /// What the rest of the machine does the first time Perch waits — for a lock,
    /// or for an answer. Once, because it stands for a thing that happens rather
    /// than for a condition: a client starting, a lock being given back, another
    /// `perch` arriving.
    pub fn once_while_waiting(self, happens: impl Fn(&FakeHost) + 'static) -> Self {
        *self.stall.somebody_else.borrow_mut() = Some(Box::new(happens));
        self
    }

    /// The same, once this many requests have gone out: a second Watcher taking
    /// the watch over between two reads of a burst, which is the one thing a
    /// wait cannot stand in for because a burst takes none.
    pub fn once_after_requests(self, sent: usize, happens: impl Fn(&FakeHost) + 'static) -> Self {
        *self.stall.somebody_else_after_requests.borrow_mut() = Some((sent, Box::new(happens)));
        self
    }

    // ---- inspecting what happened --------------------------------------

    /// Whether anything has taken Ctrl-C over from the default handler, which
    /// is what makes a loop something that can be stopped rather than killed.
    pub fn is_listening_for_interrupts(&self) -> bool {
        *self.waiting.listening.borrow()
    }

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
        self.network
            .sent
            .borrow()
            .iter()
            .filter(|request| request.url == url)
            .cloned()
            .collect()
    }

    pub fn file(&self, path: impl AsRef<Path>) -> Option<String> {
        self.fs.files.borrow().get(path.as_ref()).cloned()
    }

    /// Every file at or below a path, for a test that has to say what a whole
    /// directory tree holds.
    #[allow(
        clippy::disallowed_methods,
        reason = "the keys of the fake's own map are the tree, spelled as they were written"
    )]
    pub fn paths_under(&self, root: impl AsRef<Path>) -> Vec<PathBuf> {
        self.fs
            .files
            .borrow()
            .keys()
            .filter(|path| path.starts_with(root.as_ref()))
            .cloned()
            .collect()
    }

    /// The link a path holds, of whatever kind, and what it stands for — hard
    /// links included, which [`Links::link_target`] cannot report because the
    /// filesystem cannot either.
    pub fn link_at(&self, path: impl AsRef<Path>) -> Option<(Link, PathBuf)> {
        self.fs.links.borrow().get(path.as_ref()).cloned()
    }

    /// The permissions a path ended up with, so a test can say that a file
    /// holding a Credential was created for its owner alone.
    pub fn mode_of(&self, path: impl AsRef<Path>) -> Option<u32> {
        self.fs.modes.borrow().get(path.as_ref()).copied()
    }

    /// What the user was told that they did not ask about, in order and
    /// without repeats.
    pub fn notes(&self) -> Vec<String> {
        self.terminal.notes.borrow().clone()
    }

    /// The same as [`FakeHost::forget_effects`], for remarks: a test running two
    /// commands and asserting on the second one's should not have to read past
    /// the first one's.
    pub fn forget_notes(&self) {
        self.terminal.notes.borrow_mut().clear();
    }

    pub fn keychain_item(&self, service: &str, account: &str) -> Option<String> {
        self.keys
            .keychain
            .borrow()
            .get(&(service.to_string(), account.to_string()))
            .cloned()
    }

    /// Every service name the keychain holds an item under, so a test can say
    /// that a Profile Perch abandoned left nothing behind.
    pub fn keychain_services(&self) -> Vec<String> {
        self.keys
            .keychain
            .borrow()
            .keys()
            .map(|(service, _)| service.clone())
            .collect()
    }

    /// Creates a directory and those above it, giving `mode` to the ones that
    /// were not already there — as `mkdir -p` does. A link counts as what it
    /// points at, which is `create_dir_all`'s behavior: at a link to a directory
    /// it succeeds and uses the target, and at a link to nothing it fails,
    /// because `mkdir` answers `EEXIST` and the `is_dir` after it finds nothing.
    fn make_dirs(&self, path: &Path, mode: u32) -> Result<(), HostError> {
        // Already a directory, by whatever route — including through a link, in
        // which case the target is what was asked for and nothing is made.
        if let Some(at) = self.resolved(path) {
            return if self.fs.dirs.borrow().contains(&at) {
                Ok(())
            } else {
                Err(HostError::Other(format!(
                    "{} is already there and is not a directory",
                    path.display()
                )))
            };
        }

        // Nothing resolves. A link at the name itself is the dangling case, and
        // the one `mkdir -p` refuses rather than replaces.
        if self.fs.links.borrow().contains_key(path) {
            return Err(HostError::Other(format!(
                "{} is a link to nothing, so a directory cannot be made there",
                path.display()
            )));
        }

        // Whatever the parents resolve to is where this is made, so a directory
        // above it that is a link is followed rather than shadowed.
        let at = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                self.make_dirs(parent, mode)?;
                self.resolved(parent)
                    .unwrap_or_else(|| parent.to_path_buf())
                    .join(path.file_name().unwrap_or(path.as_os_str()))
            }
            _ => path.to_path_buf(),
        };

        self.fs.modes.borrow_mut().entry(at.clone()).or_insert(mode);
        self.fs.dirs.borrow_mut().insert(at);
        Ok(())
    }

    /// What a path names once its links are followed, or `None` when it names
    /// nothing — a link whose target has gone, most of all. The directories above
    /// it are followed too: reading a marker under a `sessions` that is a link
    /// into another Profile is reading a file inside a linked directory. Bounded,
    /// because two links pointing at each other are a loop.
    fn resolved(&self, path: &Path) -> Option<PathBuf> {
        const FOLLOWED: usize = 8;

        // A real filesystem walks `..` rather than storing it, so a path
        // carrying one names the place it lands on here too.
        let mut at = crate::host::flattened(path);
        for _ in 0..FOLLOWED {
            if self.fs.files.borrow().contains_key(&at) || self.fs.dirs.borrow().contains(&at) {
                return Some(at);
            }
            if let Some(target) = self
                .fs
                .links
                .borrow()
                .get(&at)
                .map(|(_, target)| target.clone())
            {
                at = crate::host::flattened(&crate::host::against(&at, target));
                continue;
            }
            // Nothing of that name, which does not mean nothing is there: a
            // directory *above* it may be a link. A rejoin that changes nothing
            // is a path that was never going to resolve.
            let rejoined = self.resolved(at.parent()?)?.join(at.file_name()?);
            if rejoined == at {
                return None;
            }
            at = rejoined;
        }
        None
    }

    /// The path a read lands on, or the refusal it meets on the way. Every read
    /// `RealHost` makes follows a symbolic link, so every read here does too —
    /// and a path arranged as unreadable refuses whether it is the link or what
    /// the link points at, because on a real machine either stops the read.
    fn through_links(&self, path: &Path) -> Result<PathBuf, HostError> {
        let at = self.resolved(path).unwrap_or_else(|| path.to_path_buf());
        for named in [path, at.as_path()] {
            if let Some(detail) = self.fs.unreadable.borrow().get(named) {
                return Err(HostError::Other(detail.clone()));
            }
        }
        Ok(at)
    }

    fn record(&self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
    }

    /// Passes the time the person at the terminal takes over an answer, and lets
    /// whatever the test said happens while Perch waits happen. Shared by both
    /// prompts, because a question is a wait whether or not the terminal shows
    /// what is typed — and the passphrase prompts are the longest wait in Perch,
    /// since there are two and both are somebody typing carefully.
    fn while_they_answer(&self) {
        let taken = *self.terminal.answering_takes_millis.borrow();
        if taken == 0 {
            return;
        }
        let thought_about_it =
            *self.stall.now.borrow() + chrono::Duration::milliseconds(taken as i64);
        *self.stall.now.borrow_mut() = thought_about_it;
        self.somebody_else_arrives();
    }

    /// The dialog a keychain puts in front of somebody before it lets a write
    /// through, and a wait like any other: what the test said happens while Perch
    /// waits happens here, which is the point of a stall this long.
    fn while_the_keychain_asks(&self) {
        let asked = *self.keys.keychain_set_takes_millis.borrow();
        if asked == 0 {
            return;
        }
        let answered = *self.stall.now.borrow() + chrono::Duration::milliseconds(asked as i64);
        *self.stall.now.borrow_mut() = answered;
        self.somebody_else_arrives();
    }

    /// Lets whatever the test said happens while Perch waits happen, once.
    /// Called from all four sites that move the clock, which is why `now` and
    /// `somebody_else` are one [`Stall`]. Taken out before it runs, so it can
    /// reach back into the fake without meeting a borrow the call it interrupts
    /// is still holding.
    fn somebody_else_arrives(&self) {
        let happens = self.stall.somebody_else.borrow_mut().take();
        if let Some(happens) = happens {
            happens(self);
        }
    }

    /// The same, for what the test hung on a count of requests rather than on a
    /// wait. Taken out before it runs, for [`FakeHost::somebody_else_arrives`]'s
    /// reason.
    fn somebody_else_arrives_after_enough_requests(&self) {
        let enough = self
            .stall
            .somebody_else_after_requests
            .borrow()
            .as_ref()
            .is_some_and(|(after, _)| self.network.sent.borrow().len() >= *after);
        if !enough {
            return;
        }
        let happens = self.stall.somebody_else_after_requests.borrow_mut().take();
        if let Some((_, happens)) = happens {
            happens(self);
        }
    }

    fn mark_written(&self, path: &Path) {
        self.fs
            .modified
            .borrow_mut()
            .insert(path.to_path_buf(), *self.stall.now.borrow());
    }

    /// Where a write physically lands, which is not always where it was
    /// addressed: a directory *above* the file may be a link, and a real
    /// filesystem follows it. [`resolved`](FakeHost::resolved) answers this for
    /// a path that already exists, and a write is the one case where it does not
    /// — so the parent is resolved and the name put back on the end.
    fn lands_at(&self, path: &Path) -> PathBuf {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return path.to_path_buf();
        };
        match self.resolved(parent) {
            Some(at) => at.join(name),
            None => path.to_path_buf(),
        }
    }

    /// The file a write is *for*: the path itself, or — for the copy written
    /// beside a target by [`super::replace_via_tmp`] — the target it is about to
    /// be renamed over. A test cannot name that copy, because it carries the pid
    /// of whoever is writing it, and the arrangement is about the disk and the
    /// directory rather than about a filename.
    fn intended(&self, path: &Path) -> PathBuf {
        let suffix = super::temp_suffix(self.process_id());
        match path
            .as_os_str()
            .to_str()
            .and_then(|at| at.strip_suffix(&suffix))
        {
            Some(target) => PathBuf::from(target),
            None => path.to_path_buf(),
        }
    }

    /// What a path actually ends up holding, which is what was written to it
    /// unless a test arranged otherwise.
    fn as_stored(&self, path: &Path, contents: &str) -> String {
        if self.fs.corrupting.borrow().contains(path) {
            return a_prefix_of(contents, contents.len() / 2);
        }
        contents.to_string()
    }

    /// Why a keychain call cannot be answered, when it cannot.
    ///
    /// Off macOS that is the ordinary case rather than an arrangement:
    /// `/usr/bin/security` is a Mac binary, and a fake that pretended otherwise
    /// would let a test pass on Linux for a reason that does not exist there.
    fn lock_error(&self) -> Option<KeychainError> {
        if self.platform() != Platform::MacOs && !*self.keys.keychain_everywhere.borrow() {
            return Some(KeychainError::Unavailable {
                detail: "could not run /usr/bin/security: No such file or directory".to_string(),
            });
        }
        self.keys
            .keychain_lock
            .borrow()
            .as_ref()
            .map(|lock| KeychainError::Unavailable {
                detail: lock.detail.clone(),
            })
    }

    /// The same, for a call naming one item: only an item that is found meets
    /// the lock ([`crate::keychain::EXIT_ITEM_NOT_FOUND`]). A machine with no
    /// keychain keeps the error whatever the name, having nothing to match on.
    fn lock_error_for(&self, service: &str, account: &str) -> Option<KeychainError> {
        let error = self.lock_error()?;
        let there_is_a_keychain =
            self.platform() == Platform::MacOs || *self.keys.keychain_everywhere.borrow();
        let held = self
            .keys
            .keychain
            .borrow()
            .contains_key(&(service.to_string(), account.to_string()));
        match there_is_a_keychain && !held {
            true => None,
            false => Some(error),
        }
    }
}

/// As much of `text` as fits in `bytes`, cut at a character boundary. One
/// function for the three things the fake stands in for by keeping a prefix — a
/// store that corrupts, a disk that fills partway, a keychain that takes only so
/// much — and a boundary, because `String::truncate` panics on an index that is
/// not one and what is being cut is registry JSON and `.claude.json`.
fn a_prefix_of(text: &str, bytes: usize) -> String {
    text.char_indices()
        .take_while(|(at, _)| *at < bytes)
        .map(|(_, c)| c)
        .collect()
}

fn exec_key(program: &str, args: &[&str]) -> String {
    let mut key = program.to_string();
    for arg in args {
        key.push(' ');
        key.push_str(arg);
    }
    key
}

impl port::Clock for FakeHost {
    fn now(&self) -> DateTime<Utc> {
        *self.stall.now.borrow()
    }
}

impl port::Environment for FakeHost {
    fn home_dir(&self) -> Result<PathBuf, HostError> {
        Ok(self.environment.home.clone())
    }

    fn current_dir(&self) -> Result<PathBuf, HostError> {
        Ok(self.environment.current_dir.borrow().clone())
    }

    fn env_var(&self, key: &str) -> Option<String> {
        // Empty filtered out, as the real Host filters it: `export
        // CLAUDE_CONFIG_DIR=` is ordinary shell state and reads as unset there,
        // and `Some("")` derives a store no real machine can produce.
        self.environment
            .vars
            .borrow()
            .get(key)
            .filter(|value| !value.is_empty())
            .cloned()
    }

    fn platform(&self) -> Platform {
        *self.environment.platform.borrow()
    }

    fn current_exe(&self) -> Result<PathBuf, HostError> {
        Ok(self.environment.current_exe.borrow().clone())
    }

    /// Nothing on Windows, whatever a test set: there is no uid there, and
    /// `service::stopping` reads `unwrap_or_default()` off this to build a
    /// launchd domain — so a fake answering one lets a case pass on an
    /// arrangement the platform does not have.
    fn user_id(&self) -> Option<u32> {
        match self.platform() {
            Platform::Windows => None,
            _ => *self.environment.user_id.borrow(),
        }
    }
}

impl port::Files for FakeHost {
    fn read_file(&self, path: &Path) -> Result<String, HostError> {
        self.record(Effect::ReadFile(path.to_path_buf()));
        let at = self.through_links(path)?;
        // Its own mode refuses the open where the directory answered the stat,
        // so this is a file `modified_at` still dates.
        for named in [path, at.as_path()] {
            if let Some(detail) = self.fs.unopenable.borrow().get(named) {
                return Err(HostError::Other(detail.clone()));
            }
        }
        // The real read is `read_to_string`, so bytes that are not UTF-8 come
        // back as `InvalidData` rather than as contents. The fake holds files
        // as `String` and could not otherwise reach that answer at all.
        if self.fs.not_text.borrow().contains(&at) {
            return Err(HostError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )));
        }
        // A directory is not an empty file. `NotFound` is load-bearing — a
        // `CredentialStore` reads it as "this store holds nothing" — and the real
        // `read_to_string` answers `EISDIR` here rather than nothing.
        if self.fs.dirs.borrow().contains(&at) {
            return Err(HostError::Io(std::io::Error::new(
                std::io::ErrorKind::IsADirectory,
                "is a directory",
            )));
        }
        self.fs
            .files
            .borrow()
            .get(&at)
            .cloned()
            .ok_or_else(|| HostError::NotFound {
                path: path.to_path_buf(),
            })
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
        // Both questions are asked of the file this one is *for*, which is the
        // same path unless it is the copy beside a target — the same directory
        // and the same disk, which is how the real machine answers too.
        let intended = self.intended(path);
        if let Some(detail) = self.fs.unwritable.borrow().get(&intended) {
            return Err(HostError::Other(detail.clone()));
        }
        // Counted against the file the write is *for*, so one logical write is
        // one tick however many the `replace_via_tmp` beside it costs.
        if let Some((left, detail)) = self.fs.unwritable_after.borrow_mut().get_mut(&intended) {
            match *left {
                0 => return Err(HostError::Other(detail.clone())),
                _ => *left -= 1,
            }
        }
        // Resolved *before* the parents are made, and made at the resolved place:
        // `resolved` looks in `dirs` before `links`, so making the addressed
        // path's first would put a directory over the link.
        let lands_at = self.lands_at(path);
        // Through `make_dirs`, as the real adapter's `create_dir_all` — which
        // answers `ENOTDIR` where an ancestor is a regular file, and is the whole
        // of what `write_atomically` meets on such a machine.
        if let Some(parent) = lands_at.parent() {
            self.make_dirs(parent, ORDINARY_DIR_MODE)?;
        }
        // A directory is not something `remove_file` takes away, so `create_new`
        // meets it and answers `EEXIST`. Refused rather than written over, or
        // one path would be a regular file and a directory at once.
        if self.fs.dirs.borrow().contains(&lands_at) {
            return Err(HostError::Other(format!(
                "{}: Is a directory (os error 21)",
                lands_at.display()
            )));
        }
        // Whatever else was at the path is taken away first, a link included,
        // because the real one leads with `remove_file` and then `create_new`.
        // Left behind, one path would be both a regular file and a symbolic link.
        self.fs.links.borrow_mut().remove(&lands_at);
        // A disk that fills partway leaves what fitted behind and then fails,
        // which is the order the real host does it in: open, `write_all`,
        // `sync_all`.
        if self.fs.filling.borrow().contains(&intended) {
            self.fs
                .files
                .borrow_mut()
                .insert(lands_at.clone(), a_prefix_of(contents, contents.len() / 2));
            self.fs.modes.borrow_mut().insert(lands_at.clone(), mode);
            self.mark_written(&lands_at);
            return Err(HostError::Other(
                "No space left on device (os error 28)".to_string(),
            ));
        }
        self.fs
            .files
            .borrow_mut()
            .insert(lands_at.clone(), self.as_stored(&intended, contents));
        self.fs.modes.borrow_mut().insert(lands_at.clone(), mode);
        self.mark_written(&lands_at);
        Ok(())
    }

    /// Written beside and moved into place, exactly as the real one is: through
    /// [`super::replace_via_tmp`] rather than straight into the map, because what
    /// this call promises is not only the bytes and the mode but never a
    /// half-written file at the path. A write that failed at the target rather
    /// than beside it is a failure the real host cannot produce.
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        self.record(Effect::WrotePrivateFile(path.to_path_buf()));
        if let Some(parent) = path.parent() {
            self.make_dirs(parent, PRIVATE_DIR_MODE)?;
        }
        super::replace_via_tmp(self, path, contents, PRIVATE_FILE_MODE)
    }

    fn create_private_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::CreatedDir(path.to_path_buf()));
        self.make_dirs(path, PRIVATE_DIR_MODE)?;
        self.mark_written(path);
        Ok(())
    }

    /// A path Perch did not create has whatever mode it was given: an ordinary
    /// one for a directory, and — since a file the fixtures put there stands in
    /// for one Claude Code wrote — the owner alone for a file.
    fn file_mode(&self, path: &Path) -> Result<Option<u32>, HostError> {
        // Through a link, as `metadata` reads through one: the mode of a
        // symbolic link is not a thing anybody asks about.
        let Some(path) = self.resolved(path).as_deref().map(Path::to_path_buf) else {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        };
        let path = path.as_path();
        // Windows has no mode and relies on the profile ACL, and the real Host
        // answers `None` there. A number here would let a test drive
        // `tighten_if_loose` on a platform where none of it happens.
        if self.platform() == Platform::Windows {
            return Ok(None);
        }
        Ok(Some(self.mode_of(path).unwrap_or(
            if self.fs.dirs.borrow().contains(path) {
                ORDINARY_DIR_MODE
            } else {
                PRIVATE_FILE_MODE
            },
        )))
    }

    /// Refusing a link, as `RealHost` does: the narrowing is an `O_NOFOLLOW` open
    /// followed by an `fchmod`, so a link at the last component fails rather than
    /// sending the mode wherever it points, while a directory *above* it is
    /// followed. Where permission bits mean nothing there is nothing to redirect,
    /// so this gates on the runtime Platform as `file_mode` does.
    fn make_private(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::MadePrivate(path.to_path_buf()));
        if self.platform() != Platform::Windows
            && self.fs.links.borrow().contains_key(&self.lands_at(path))
        {
            return Err(HostError::Io(std::io::Error::other(format!(
                "{} is a symbolic link, and narrowing one would set the mode of \
                 whatever it points at",
                path.display(),
            ))));
        }
        let Some(path) = self.resolved(path) else {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        };
        let path = path.as_path();
        // A file whose *permissions* cannot be changed is the same arrangement as
        // one whose contents cannot: a `chmod` on somebody else's file fails with
        // `EPERM` however readable it is.
        if let Some(detail) = self.fs.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.fs
            .modes
            .borrow_mut()
            .insert(path.to_path_buf(), PRIVATE_FILE_MODE);
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::CreatedDir(path.to_path_buf()));
        self.make_dirs(path, ORDINARY_DIR_MODE)?;
        self.mark_written(path);
        Ok(())
    }

    /// A link counts as what it points at, as it does on a real filesystem —
    /// so a link into a directory that has gone is a path that does not exist,
    /// which is the state a Reconcile has to notice.
    fn path_exists(&self, path: &Path) -> bool {
        self.resolved(path).is_some()
    }

    fn is_file(&self, path: &Path) -> bool {
        self.resolved(path)
            .is_some_and(|at| self.fs.files.borrow().contains_key(&at))
    }

    /// Takes a directory and everything under it, unless the directory itself is
    /// one arranged not to go. A path *inside* it that will not go is not
    /// consulted: the real call walks what is there, and a test that wants the
    /// walk to fail says so of the directory it is walking.
    fn remove_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedDir(path.to_path_buf()));
        if let Some(detail) = self.fs.undeletable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // A directory that will not be walked cannot be removed either: the
        // remove has to `opendir` first, so there is no machine where one
        // refuses `opendir` and then disappears under a recursive remove.
        if let Some(detail) = self.fs.unlistable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // The directories on the way resolve and the last component does not,
        // as [`Self::remove_file`]'s do.
        let path = &self.lands_at(path);
        // A link at the path is taken away and what it points at is left alone:
        // `remove_dir_all` does not follow the last component, so it unlinks the
        // link itself and answers `Ok`. Measured rather than assumed.
        if self.fs.links.borrow_mut().remove(path).is_some() {
            self.fs.modified.borrow_mut().remove(path);
            self.fs.files.borrow_mut().remove(path);
            self.fs.modes.borrow_mut().remove(path);
            return Ok(());
        }
        // A plain file is not a directory, and the real call says so rather than
        // removing it — a fake that removed it anyway would recover from a state
        // a real machine cannot.
        if self.fs.files.borrow().contains_key(path) {
            return Err(HostError::Other(format!(
                "{}: Not a directory (os error 20)",
                path.display()
            )));
        }
        // Prefix over the fake's own keys, and not a containment guard: this is
        // the tree walk `remove_dir_all` does, which descends into directories
        // and takes a link at a name without following it.
        #[allow(
            clippy::disallowed_methods,
            reason = "the keys of the fake's own map are the tree, spelled as they were written"
        )]
        {
            self.fs
                .dirs
                .borrow_mut()
                .retain(|dir| !dir.starts_with(path));
            self.fs
                .links
                .borrow_mut()
                .retain(|at, _| !at.starts_with(path));
            self.fs
                .files
                .borrow_mut()
                .retain(|file, _| !file.starts_with(path));
            self.fs
                .modified
                .borrow_mut()
                .retain(|written, _| !written.starts_with(path));
            self.fs
                .modes
                .borrow_mut()
                .retain(|at, _| !at.starts_with(path));
        }
        Ok(())
    }

    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError> {
        // Told apart from the path being taken: `AlreadyExists` is contention
        // and anything else is the filesystem refusing, and `lock::take` waits
        // the first out and reports the second.
        if let Some(detail) = self.fs.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // A link in the way counts, whether or not it still resolves: `mkdir`
        // does not follow the last component, so it fails `EEXIST` at a symlink
        // whatever it points at. `path_exists` answers the Reconcile's question.
        if self.fs.links.borrow().contains_key(&self.lands_at(path)) || self.path_exists(path) {
            return Err(HostError::AlreadyExists {
                path: path.to_path_buf(),
            });
        }
        // `mkdir` and not `mkdir -p`: `std::fs::create_dir` answers `ENOENT` for
        // a missing parent, so a lock cannot be taken inside a Profile that does
        // not exist.
        let parent = path.parent().filter(|at| !at.as_os_str().is_empty());
        if let Some(parent) = parent
            && self
                .resolved(parent)
                .is_none_or(|at| !self.fs.dirs.borrow().contains(&at))
        {
            return Err(HostError::NotFound {
                path: parent.to_path_buf(),
            });
        }
        // Where it lands rather than where it was addressed, as `create_file_with_mode`
        // is: one directory under two spellings is one `mkdir`, so a lock taken
        // through a link has to exclude the take that names it directly.
        let lands_at = self.lands_at(path);
        self.fs.dirs.borrow_mut().insert(lands_at.clone());
        self.mark_written(&lands_at);
        // Recorded where the lock was taken, and not before the refusals above
        // it: `Effect::Took` is the fake's word for an acquired lock, and
        // `lock::take` asks up to eight times for one it never gets.
        self.record(Effect::Took(path.to_path_buf()));
        Ok(())
    }

    /// A path arranged as unreadable will not say when it was written either —
    /// which is how "the lock is gone" and "the lock will not say" are told
    /// apart, and they are different answers.
    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError> {
        if let Some(detail) = self.fs.unreadable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // Not through a link, because a dangling one is the state
        // `lock::abandoned` has to meet: the real call answers `NotFound`
        // there, which is the arm that reads a lock as free.
        let at = self.resolved(path).unwrap_or_else(|| path.to_path_buf());
        self.fs
            .modified
            .borrow()
            .get(&at)
            .copied()
            .ok_or_else(|| HostError::NotFound {
                path: path.to_path_buf(),
            })
    }

    fn touch(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::Touched(path.to_path_buf()));
        let Some(path) = self.resolved(path) else {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        };
        let path = path.as_path();
        // A path arranged as unwritable will not take a touch either — which is
        // how `lock::renew`'s "the artifact would not take a fresh timestamp"
        // branch is reached without arranging a filesystem that misbehaves.
        if let Some(detail) = self.fs.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.mark_written(path);
        Ok(())
    }

    fn rename(&self, from: &Path, to: &Path) -> Result<(), HostError> {
        self.record(Effect::Renamed {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
        if let Some(detail) = self.fs.unwritable.borrow().get(to) {
            return Err(HostError::Other(detail.clone()));
        }
        // Both ends through any link *above* them, because a real `rename(2)`
        // resolves the directories on the way to each name and not the last
        // component — which the arm below is about.
        let from = &self.lands_at(from);
        let to = &self.lands_at(to);
        // A directory at the destination refuses on every real adapter, and the
        // fake would otherwise describe a path that is a file and a directory
        // at once. `replace_via_tmp` ends here, so this is every private write.
        if self.fs.dirs.borrow().contains(to) {
            return Err(HostError::Io(std::io::Error::from(
                std::io::ErrorKind::IsADirectory,
            )));
        }
        // `Io`, which is what the real host answers: `NotFound` is load-bearing
        // elsewhere — `CredentialStore::read` reads it as "this store holds
        // nothing" and `clients_in` as "nothing is running".
        let moved = self
            .fs
            .files
            .borrow_mut()
            .remove(from)
            .ok_or_else(|| HostError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)))?;
        self.fs.files.borrow_mut().insert(to.to_path_buf(), moved);
        // A rename moves the file, mode and all: what ends up at the target is
        // the file that was created beside it, not the one it replaced.
        let mode = self.fs.modes.borrow_mut().remove(from);
        match mode {
            Some(mode) => self.fs.modes.borrow_mut().insert(to.to_path_buf(), mode),
            None => self.fs.modes.borrow_mut().remove(to),
        };
        // The age goes with the file. Left behind, it makes `modified_at` answer
        // for a path `path_exists` already says is gone — and that answer is the
        // one `lock::abandoned` reads as "the artifact is still there".
        self.fs.modified.borrow_mut().remove(from);
        // And it replaces whatever was at the target, a link included, because
        // `rename(2)` does not follow the last component. That is the security
        // property `replace_via_tmp` exists to produce.
        self.fs.links.borrow_mut().remove(to);
        // The directories a rename's target sits in are there by the time it
        // lands. Recorded here too, so a `rename` reached any other way leaves
        // the fake describing a filesystem that could exist.
        self.note_directories_of(to);
        self.mark_written(to);
        Ok(())
    }

    /// Through the link a write went through, and never through the last
    /// component: `unlink` resolves the directories on the way and then takes
    /// away the name it was given, link or file. Without the first half a file
    /// created under a linked parent could not be removed — which is
    /// `replace_via_tmp`'s cleanup on any Profile Reconcile has been over.
    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedFile(path.to_path_buf()));
        if let Some(detail) = self.fs.undeletable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        let at = self.lands_at(path);
        // A directory is a refusal rather than a no-op: `credentials::forget`
        // reads `Ok` here as a store it emptied, and answering that for a
        // directory it did not touch reports a Credential as deleted.
        if self.fs.dirs.borrow().contains(&at) {
            return Err(HostError::Io(std::io::Error::from(
                std::io::ErrorKind::IsADirectory,
            )));
        }
        self.fs.files.borrow_mut().remove(&at);
        self.fs.links.borrow_mut().remove(&at);
        self.fs.modified.borrow_mut().remove(&at);
        self.fs.modes.borrow_mut().remove(&at);
        Ok(())
    }

    /// Through a link, as `read_dir` reads through one — but reporting what was
    /// found under the name that was asked about, which is what `read_dir` does
    /// with the path it was given. A `sessions` linked into another Profile
    /// therefore answers with that Profile's markers.
    fn list_dir(&self, asked: &Path) -> Result<Vec<PathBuf>, HostError> {
        let resolved = self.resolved(asked);
        let Some(path) = resolved
            .clone()
            .filter(|at| self.fs.dirs.borrow().contains(at))
        else {
            // `ENOTDIR` and not `ENOENT`, which are opposite answers to
            // `probe::clients_in`: one is "nothing is running" and the other is
            // doubt it refuses on.
            if resolved.is_some() {
                return Err(HostError::Other(format!(
                    "{} is not a directory",
                    asked.display()
                )));
            }
            return Err(HostError::NotFound {
                path: asked.to_path_buf(),
            });
        };
        let path = path.as_path();
        // A directory that is there and will not be read is a different answer
        // from one that is not there, and callers are entitled to tell them
        // apart. Arranged the same way an unreadable file is.
        let refused = self
            .fs
            .unreadable
            .borrow()
            .get(path)
            .cloned()
            .or_else(|| self.fs.unlistable.borrow().get(path).cloned());
        if let Some(detail) = refused {
            return Err(HostError::Other(detail));
        }
        let held = |candidate: &PathBuf| candidate.parent() == Some(path);
        let mut found: BTreeSet<PathBuf> = self
            .fs
            .files
            .borrow()
            .keys()
            .filter(|file| held(file))
            .cloned()
            .collect();
        found.extend(
            self.fs
                .dirs
                .borrow()
                .iter()
                .filter(|dir| held(dir))
                .cloned(),
        );
        // Links included, pointing at something or not: a directory holds the
        // ones it holds, and a broken one is precisely what has to be found.
        found.extend(self.fs.links.borrow().keys().filter(|at| held(at)).cloned());
        // Under the name that was asked about rather than the one it resolves
        // to, so a caller that joins a name onto what it was given finds it.
        Ok(found
            .into_iter()
            .map(|at| match at.file_name() {
                Some(name) => asked.join(name),
                None => at,
            })
            .collect())
    }
}

impl port::Links for FakeHost {
    /// Makes a link, refusing the kinds this machine could not make — which is
    /// the point of having this behind the port at all: a symbolic link on a
    /// Windows without Developer Mode, and a junction anywhere else, fail here
    /// exactly as they would there, so the fallbacks are exercised from whatever
    /// machine the tests run on.
    fn link(&self, kind: Link, target: &Path, at: &Path) -> Result<(), HostError> {
        self.record(Effect::Linked {
            kind,
            target: target.to_path_buf(),
            at: at.to_path_buf(),
        });
        if let Some(detail) = self.fs.unwritable.borrow().get(at) {
            return Err(HostError::Other(detail.clone()));
        }
        if self.fs.links.borrow().contains_key(at) || self.path_exists(at) {
            return Err(HostError::AlreadyExists {
                path: at.to_path_buf(),
            });
        }
        // A link is made inside a directory, and `symlink(2)` answers `ENOENT`
        // where there is none. Refused rather than invented, or a caller that
        // forgot to make the Profile first passes here and fails on a machine.
        if let Some(parent) = at.parent().filter(|at| !at.as_os_str().is_empty())
            && !self
                .resolved(parent)
                .is_some_and(|at| self.fs.dirs.borrow().contains(&at))
        {
            // `Io` with a `NotFound` kind, which is what `RealHost` hands back:
            // `symlink` reports through `std::io::Error` and nothing narrows it.
            return Err(HostError::Io(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        }

        let windows = self.platform() == Platform::Windows;
        match kind {
            Link::Symbolic if windows && !*self.fs.developer_mode.borrow() => {
                return Err(HostError::Other(
                    "A required privilege is not held by the client. (os error 1314)".to_string(),
                ));
            }
            Link::Junction if !windows => {
                return Err(port::junctions_are_windows_only());
            }
            // A hard link is a second name for a *file*: there is no such thing
            // as one for a directory, and nothing to name where nothing is.
            Link::Hard if !self.is_file(target) => {
                return Err(HostError::Other(format!(
                    "{} is not a file, so it has no second name to give",
                    target.display()
                )));
            }
            _ => {}
        }

        // Keyed where the link lands, as `symlink(2)` makes it and
        // `link_target` reads it back: the directories above resolve and the
        // last component does not.
        let at = &self.lands_at(at);
        self.fs
            .links
            .borrow_mut()
            .insert(at.to_path_buf(), (kind, target.to_path_buf()));
        if kind == Link::Hard {
            // The file is now reachable under both names, and nothing about the
            // second one says it is a second one.
            let contents = self.file(target).unwrap_or_default();
            self.set_file(at, &contents);
        }
        self.note_directories_of(at);
        // Only a hard link, which is a second name for the file and shares its
        // modification time. A symbolic link has none of its own, so a dangling
        // one is the `NotFound` that reads as a lock nobody holds.
        if kind == Link::Hard {
            self.mark_written(at);
        }
        Ok(())
    }

    fn link_target(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        // Through the directories above and never the last component, as
        // `symlink_metadata` reads a path: a Profile `reconcile` has been over
        // has a linked directory in front of every name it then asks about.
        let path = &self.lands_at(path);
        match self.fs.links.borrow().get(path) {
            // A hard link tells nothing about itself, so it answers as the
            // ordinary file it is indistinguishable from.
            Some((Link::Hard, _)) | None => {
                if self.fs.files.borrow().contains_key(path) || self.fs.dirs.borrow().contains(path)
                {
                    Ok(None)
                } else {
                    Err(HostError::NotFound {
                        path: path.to_path_buf(),
                    })
                }
            }
            Some((_, target)) => Ok(Some(target.clone())),
        }
    }

    fn remove_link(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedLink(path.to_path_buf()));
        if let Some(detail) = self.fs.undeletable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // Where the link lands rather than where it was addressed, as `link`
        // keyed it and `link_target` reads it back: a Profile a Reconcile has
        // been over has a linked directory in front of every name after it.
        let path = &self.lands_at(path);
        // A hard link is another name for the file and says nothing about
        // itself, so neither real adapter can recognize one: unix asks
        // `is_symlink` and Windows asks for a reparse point, and both refuse.
        if matches!(self.fs.links.borrow().get(path), Some((Link::Hard, _))) {
            return Err(port::not_a_link(path));
        }
        if self.fs.links.borrow_mut().remove(path).is_some() {
            self.fs.files.borrow_mut().remove(path);
            self.fs.modes.borrow_mut().remove(path);
            self.fs.modified.borrow_mut().remove(path);
            return Ok(());
        }
        // Something that is not a link is refused rather than passed over, as
        // the real call refuses it: answered `Ok`, this could not tell a caller
        // that dropped its `link_target` guard from one that kept it.
        if self.fs.files.borrow().contains_key(path) || self.fs.dirs.borrow().contains(path) {
            return Err(port::not_a_link(path));
        }
        self.fs.modified.borrow_mut().remove(path);
        Ok(())
    }
}

impl port::Keys for FakeHost {
    fn keychain_get(&self, service: &str, account: &str) -> Result<String, KeychainError> {
        self.record(Effect::KeychainGet {
            service: service.to_string(),
            account: account.to_string(),
        });
        // A read stalls as a write does: `security find-generic-password -w`
        // against an item this binary did not create is the classic prompt, and
        // `switch::capture` reads inside `Holds::around` for exactly that.
        self.while_the_keychain_asks();
        if let Some(error) = self.lock_error_for(service, account) {
            return Err(error);
        }
        self.keys
            .keychain
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
        // What `security -i` will not take, asked here as the real adapter asks
        // it through `add_command_line`: the account name is `$USER` verbatim,
        // so a fake accepting one no machine would proves nothing.
        crate::keychain::storable(service, account)?;
        self.while_the_keychain_asks();
        if let Some(error) = self.lock_error() {
            return Err(error);
        }
        // Truncated at a byte boundary and never mid-character: what is being
        // stood in for is a buffer that stops, not a broken encoder.
        let kept = match *self.keys.keychain_keeps.borrow() {
            Some(bytes) if bytes < secret.len() => a_prefix_of(secret, bytes),
            _ => secret.to_string(),
        };
        self.keys
            .keychain
            .borrow_mut()
            .insert((service.to_string(), account.to_string()), kept);
        Ok(())
    }

    fn keychain_delete(&self, service: &str, account: &str) -> Result<(), KeychainError> {
        self.record(Effect::KeychainDelete {
            service: service.to_string(),
            account: account.to_string(),
        });
        self.while_the_keychain_asks();
        if let Some(error) = self.lock_error_for(service, account) {
            return Err(error);
        }
        self.keys
            .keychain
            .borrow_mut()
            .remove(&(service.to_string(), account.to_string()))
            .map(|_| ())
            .ok_or_else(|| KeychainError::NotFound {
                service: service.to_string(),
                account: account.to_string(),
            })
    }
}

/// Whether a program is named as a path rather than as a word `PATH` answers.
///
/// Off the platform the Host reports rather than through `Path::parent`, for
/// [`crate::probe::rooted`]'s reason: a backslash separates on Windows alone, so
/// a fake claiming it must not answer by the runner it is on.
fn names_a_place(program: &str, on_windows: bool) -> bool {
    program.contains('/') || (on_windows && program.contains('\\'))
}

impl port::Processes for FakeHost {
    fn exec(&self, program: &str, args: &[&str]) -> Result<Execution, HostError> {
        self.record(Effect::Exec {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        });
        self.processes
            .executions
            .borrow()
            .get(&exec_key(program, args))
            .cloned()
            .ok_or_else(|| HostError::Other(format!("no such program: {program}")))
    }

    /// Stands in for whatever took the terminal — the login the user drives, or
    /// the program a Run launched. A program [`FakeHost::set_exec`] has an answer
    /// for exits with that answer's status, ahead of the [`Login`], because *the
    /// program took the terminal and refused* is what `brew upgrade perch` and an
    /// installer script do and neither has a directory to write into.
    fn exec_interactive(
        &self,
        program: &str,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> Result<i32, HostError> {
        let config_dir = env
            .iter()
            .find(|(key, _)| *key == "CLAUDE_CONFIG_DIR")
            .map(|(_, value)| PathBuf::from(value))
            .unwrap_or_else(|| self.environment.home.join(".claude"));

        self.record(Effect::ExecInteractive {
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            config_dir: config_dir.clone(),
            env: env
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
        });

        if let Some(arranged) = self
            .processes
            .executions
            .borrow()
            .get(&exec_key(program, args))
        {
            return Ok(arranged.status);
        }

        // A program the machine does not hold is one no machine launches: the
        // real adapter fails at `spawn`, and a `0` here is a Run reporting a
        // success it never had. A bare word is `PATH`'s answer, not this one's.
        if names_a_place(program, self.platform() == Platform::Windows)
            && !self.path_exists(Path::new(program))
        {
            return Err(HostError::NotFound {
                path: PathBuf::from(program),
            });
        }

        let login = self.processes.login.borrow();
        match login.as_ref() {
            Some(login) => Ok(login(self, &config_dir)),
            None => Ok(0),
        }
    }

    fn process_id(&self) -> u32 {
        THIS_PROCESS
    }

    fn process_alive(&self, pid: u32) -> bool {
        port::is_a_pid(pid) && self.processes.live_processes.borrow().contains_key(&pid)
    }

    /// Guarded as [`Self::process_alive`] is, and for the same reason: a fake
    /// that answers a start time for a number that names no process describes a
    /// machine neither adapter runs on.
    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>> {
        port::is_a_pid(pid)
            .then(|| {
                self.processes
                    .live_processes
                    .borrow()
                    .get(&pid)
                    .copied()
                    .flatten()
            })
            .flatten()
    }
}

impl port::Waiting for FakeHost {
    /// Costs no time, but does pass it: waiting for a lock somebody else holds
    /// is how that lock comes to be stale, and a test should be able to reach
    /// that without sitting through it.
    fn sleep(&self, millis: u64) {
        self.record(Effect::Slept { millis });
        let waited = *self.stall.now.borrow() + chrono::Duration::milliseconds(millis as i64);
        *self.stall.now.borrow_mut() = waited;
        self.somebody_else_arrives();
    }

    fn listen_for_interrupts(&self) {
        *self.waiting.listening.borrow_mut() = true;
    }

    /// The same question [`FakeHost::wait`] answers, without the wait: asked
    /// once either count the test set has been reached, and never where nothing
    /// is listening.
    fn asked_to_stop(&self) -> bool {
        if !*self.waiting.listening.borrow() {
            return false;
        }
        let waited_enough = matches!(
            *self.waiting.interrupt_after.borrow(),
            Some(after) if *self.waiting.waits.borrow() >= after
        );
        let sent_enough = matches!(
            *self.waiting.interrupt_after_requests.borrow(),
            Some(after) if self.network.sent.borrow().len() >= after
        );
        waited_enough || sent_enough
    }

    /// Passes the time the same way a sleep does, and ends the way the test
    /// said the person watching would end it.
    fn wait(&self, millis: u64) -> Waited {
        self.record(Effect::Waited { millis });

        // Decided before the clock moves, because the real one decides before it
        // sleeps and returns having spent nothing. Nothing is interrupted where
        // nothing is listening: Ctrl-C ends the process instead.
        let interrupted = {
            let mut waits = self.waiting.waits.borrow_mut();
            *waits += 1;
            match *self.waiting.interrupt_after.borrow() {
                Some(after) => *self.waiting.listening.borrow() && *waits >= after,
                None => false,
            }
        };

        if !interrupted {
            let waited = *self.stall.now.borrow() + chrono::Duration::milliseconds(millis as i64);
            *self.stall.now.borrow_mut() = waited;
        }

        // Run either way, which is the one site where that is a decision: what
        // the closure stands for does not stop happening because this Perch was
        // interrupted, so this is where the clock stands still and it runs.
        self.somebody_else_arrives();

        match interrupted {
            true => Waited::Interrupted,
            false => Waited::Fully,
        }
    }
}

impl port::Terminal for FakeHost {
    fn is_interactive(&self) -> bool {
        *self.terminal.interactive.borrow()
    }

    /// Kept as the real one writes it, stripped and all: a fake that recorded
    /// the raw line is one no case could read the rule off.
    fn note(&self, line: &str) {
        let line = crate::host::Shown::in_prose(line).as_str().to_string();
        let mut notes = self.terminal.notes.borrow_mut();
        if !notes.contains(&line) {
            notes.push(line);
        }
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        self.record(Effect::Asked);
        self.while_they_answer();
        Ok(self.terminal.answers.borrow_mut().pop_front())
    }

    fn read_secret(&self) -> Result<Option<Zeroizing<String>>, HostError> {
        self.record(Effect::AskedInSecret);
        self.while_they_answer();
        Ok(self
            .terminal
            .secrets
            .borrow_mut()
            .pop_front()
            .map(Zeroizing::new))
    }
}

impl port::Network for FakeHost {
    /// Answers with whatever the test arranged for this endpoint, and nothing at
    /// all otherwise — so a command that fetches when it should not fails rather
    /// than quietly succeeding. Every request is recorded either way.
    fn http(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, HostError> {
        // Asked before the arrangement is consulted, because the real adapter
        // asks before it spawns `curl`: a request it would refuse must not be
        // one a suite can answer.
        crate::host::sendable(request)?;

        let sent = Sent {
            url: request.url.to_string(),
            headers: request
                .headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            body: request.body.map(str::to_string),
            within_millis: request.within_millis,
        };
        self.record(Effect::Http {
            url: sent.url.clone(),
        });
        let asked = sent.url.clone();
        let bearer = sent.bearer().map(str::to_string);
        self.network.sent.borrow_mut().push(sent);
        self.somebody_else_arrives_after_enough_requests();

        // Bounded as `curl` is bounded, by the request's own `max-time` or by
        // the ceiling every request without one gets: a reply arriving after
        // that does not arrive, so a fixture arranging one arranges no machine.
        let takes = *self.network.takes_millis.borrow();
        let bound = request.bound_millis();
        if takes > 0 {
            let waited = takes.min(bound);
            let answered = *self.stall.now.borrow() + chrono::Duration::milliseconds(waited as i64);
            *self.stall.now.borrow_mut() = answered;
            self.somebody_else_arrives();
        }
        if takes > bound {
            return Err(HostError::Other(format!(
                "curl exited 28: Operation timed out after {bound} milliseconds"
            )));
        }

        // A trace answers before a fixed reply does, and its last entry stays
        // put: a figure that moves is what the test is about, and one that has
        // stopped moving still has to answer.
        if let Some(trace) = self
            .network
            .traces
            .borrow_mut()
            .get_mut(&(asked.clone(), bearer.clone()))
        {
            let answered = match trace.len() {
                0 | 1 => trace.front().cloned(),
                _ => trace.pop_front(),
            };
            if let Some(answered) = answered {
                return Ok(answered);
            }
        }

        let replies = self.network.replies.borrow();
        replies
            .get(&(asked.clone(), bearer))
            .or_else(|| replies.get(&(asked.clone(), None)))
            .cloned()
            .ok_or_else(|| HostError::Other(format!("the fake Host has no network: {asked}")))
    }
}

impl port::Filesystem for FakeHost {}

impl port::Host for FakeHost {}

#[cfg(test)]
mod tests {
    use super::*;
    // The one concern these tests reach that the file's own body does not.
    use super::port::Links as _;

    /// An access token comes out of a file Perch does not author, so one
    /// carrying a newline is a request the real adapter refuses before it
    /// spawns `curl`. A fake that answered it would prove a Refresh, a Back-off
    /// and a Quarantine decision against a request no machine would make.
    // The port method itself is what this asserts on, so it is the one call the
    // way through would defeat.
    #[allow(
        clippy::disallowed_methods,
        reason = "the claim is about the port's own method rather than about a request Perch makes"
    )]
    #[test]
    fn a_request_the_real_adapter_would_not_send_is_not_one_the_fake_answers() {
        use super::port::Network as _;

        let host = FakeHost::new().with_reply("https://example.test/usage", 200, "{}");

        for refused in [
            HttpRequest::get(
                "https://example.test/usage",
                &[("Authorization", "Bearer sk-ant\noutput = /tmp/taken")],
            ),
            HttpRequest::post("https://example.test/usage", &[], "@/etc/passwd"),
        ] {
            assert!(
                host.http(&refused).is_err(),
                "the fake answered what the real adapter refuses"
            );
        }
    }

    /// On macOS the permission dialog is put up for a *read* as well as a write:
    /// `security find-generic-password -w` against an item this binary did not
    /// create is the classic one. A fake that stalled only on writes made the
    /// read-side `Holds::around` — which `switch::resolve_a_landing` needs —
    /// unreachable by any fixture.
    #[test]
    fn a_keychain_read_stalls_for_the_dialog_as_a_write_does() {
        use super::port::{Clock as _, Keys as _};

        let host = FakeHost::new().with_a_keychain_that_asks_first(20_000);
        host.set_keychain_item("service", "account", "secret");
        let before = host.now();

        host.keychain_get("service", "account")
            .expect("it is there");

        assert_eq!(
            host.now() - before,
            chrono::Duration::milliseconds(20_000),
            "the clock moves by what the person took to answer"
        );
    }

    /// The fixture is a Profile holding a link into a Default Profile that has
    /// gone, which `reconcile::sweep` exists to prevent and a machine can still
    /// arrive at. There a real `lock::take` gets `AlreadyExists`, reads the lock
    /// as abandoned because a dangling link will not say when it was written, and
    /// refuses every Switch against that Profile for ever.
    #[test]
    fn a_directory_is_not_taken_over_a_link_that_points_at_nothing() {
        let host = FakeHost::new().with_link(
            Link::Symbolic,
            "/Users/someone/.claude/.oauth_refresh.lock",
            "/Users/someone/.config/perch/profiles/a/.oauth_refresh.lock",
        );
        let at = Path::new("/Users/someone/.config/perch/profiles/a/.oauth_refresh.lock");

        // The target never existed, so the link dangles.
        assert!(!host.path_exists(at), "the link resolves to nothing");

        assert!(
            matches!(
                host.create_dir_exclusive(at),
                Err(HostError::AlreadyExists { .. })
            ),
            "a link in the way is in the way, as it is to a real `mkdir`"
        );
    }

    /// `rename(2)` does not follow the last component either, which is the whole
    /// of why `replace_via_tmp` is the write a Credential goes through: a link
    /// planted at `.credentials.json` is *replaced* rather than followed to
    /// wherever whoever planted it wanted the secret to land.
    #[test]
    fn a_private_write_over_a_planted_link_replaces_it_rather_than_following_it() {
        let planted = Path::new("/Users/someone/.config/perch/profiles/a/.credentials.json");
        let elsewhere = Path::new("/tmp/somewhere-a-stranger-can-read");
        let host = FakeHost::new().with_link(Link::Symbolic, elsewhere, planted);

        host.write_private_file(planted, "a refresh token")
            .expect("the write lands");

        assert_eq!(
            host.link_target(planted).expect("the path answers"),
            None,
            "the link is gone, because a rename replaced it"
        );
        assert_eq!(
            host.read_file(planted).ok().as_deref(),
            Some("a refresh token"),
            "and the Credential is at the path Perch chose"
        );
        assert!(
            !host.path_exists(elsewhere),
            "rather than at the one the link named"
        );
    }
    /// The mirror of `real.rs`'s
    /// `a_process_id_that_is_not_one_is_dead_rather_than_a_process_group`:
    /// `clients_in` parses a pid out of any filename in a sessions directory, and
    /// those are not names Perch wrote, so `0.json` is reachable.
    #[test]
    fn the_process_ids_that_are_not_one_are_dead_here_too() {
        let host = FakeHost::new()
            .with_live_process(0)
            .with_live_process(u32::MAX);

        assert!(
            !host.process_alive(0),
            "0 is a process group, not a process"
        );
        assert!(
            !host.process_alive(u32::MAX),
            "4294967295 narrows to -1, which is every process the caller may signal"
        );
    }
}
