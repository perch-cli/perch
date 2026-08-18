//! A Host that keeps the world in memory and records what it was asked to do.
//!
//! Behavior tests drive real command code against this and assert on
//! observable outcomes: what was printed, what ended up in the keychain, and
//! what went out to the network. A machine with no arranged replies has no
//! network at all, so a command that fetches when it should not fails here
//! rather than quietly passing (ADR 0015).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};

// The port under a name of its own, because all nine of its trait names are
// also the names of the concerns whose state is held below — and the state is
// what a reader of this file is usually looking at. `impl port::Files for
// FakeHost` says *this is the surface*; the bare `Filesystem` says *this is the
// world*. Naming the state anything else would mint a second vocabulary for the
// nine things ADR 0056 spent its effort naming once.
use super as port;
use super::{
    Execution, HostError, HttpRequest, HttpResponse, Link, PRIVATE_DIR_MODE, PRIVATE_FILE_MODE,
    Platform, Waited,
};
// The methods, without the names: every `self.platform()`, `self.process_id()`
// and `self.path_exists()` below is a trait method, and a trait has to be in
// scope to be found even where its name is not wanted (`host::prelude`'s
// reason, ADR 0056). Through `port` rather than `super`, because two of these
// three names are also the state's and only one spelling of the port belongs in
// this file.
use crate::keychain::KeychainError;
use port::{Environment as _, Files as _, Processes as _};

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
    /// questions: how long Perch spent waiting for somebody else's lock, and
    /// how many times round the loop went.
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
        /// the embedded installer which Release to fetch (ADR 0039) — and a
        /// tag that never reached the script is an upgrade to the wrong thing,
        /// which nothing else here would notice.
        env: Vec<(String, String)>,
    },
    /// A request that went out to the network.
    Http {
        url: String,
    },
    /// A question put to the person at the terminal. Recorded so a command that
    /// must never ask one — bare `perch switch` (ADR 0011) — can be held to it.
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
    /// only looks at where it went — and the whole point of the check on
    /// `perch --version` is that it costs nothing on a machine that cannot
    /// answer (ADR 0039).
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
/// Perch waits in two places — contending for a lock, and putting a question to
/// the person at the terminal — and the world does not stand still through
/// either: a `claude` started during a lock wait is exactly what the questions
/// asked *under* the locks are asked again for, and another `perch` claiming an
/// abandoned lock while somebody stares at a `[y/N]` is what the hold is
/// re-checked afterwards for. Without this the fake can only present a world
/// that was already settled before the command began.
pub type WhileWaiting = Box<dyn Fn(&FakeHost)>;

/// The replies an endpoint gives one asker, in the order it gives them: keyed
/// the way a single reply is, by URL and by the access token that asked.
type Traces = BTreeMap<(String, Option<String>), VecDeque<HttpResponse>>;

/// The world, held in the pieces the port names it in.
///
/// Seven concern structs, the [`Stall`], and the recorder. They are *state and
/// nothing else*: every method stays on `FakeHost`, so a `port::Files` method
/// that has to touch the clock writes `self.stall.now` and `self.fs.modified`
/// in the same breath, and the private helpers that straddle concerns —
/// `record`, `mark_written`, `lock_error`, `while_they_answer`, `intended` —
/// stay where they can reach everything. Even so, three of the nine `impl`
/// blocks reach a struct that is not their own — `port::Keys` and
/// `port::Waiting` both take the [`Stall`], and `port::Processes` reads
/// `environment.home` — and the helpers carry the rest of the crossings that
/// were ten before they had anywhere to live. That is ADR 0056's thesis
/// arriving in the state: the machine's surfaces are entangled, and a copy of
/// the world is entangled in the same places. Methods on these structs would
/// turn every one of those crossings into real coupling — passing a clock into
/// the filesystem — which ADR 0056 already refused at the trait level
/// (ADR 0059).
///
/// The `RefCell` stays on each field rather than moving out to wrap the struct.
/// The fake is deliberately re-entered mid-call by a `somebody_else` closure,
/// and eight struct-wide borrows where there were thirty-nine field-wide ones
/// would turn that documented safety into `already borrowed` at runtime.
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
    /// concerns and read back whole through [`FakeHost::effects`], so it is
    /// cross-cutting on purpose and stays one bare field.
    effects: RefCell<Vec<Effect>>,
}

/// The machine Perch was started on: where it runs, as whom, and as what.
struct Environment {
    home: PathBuf,
    /// The directory the command was typed in — the project a Run is about,
    /// and the one entry of `projects` that crosses to a Profile (ADR 0003).
    current_dir: RefCell<PathBuf>,
    platform: RefCell<Platform>,
    vars: RefCell<BTreeMap<String, String>>,
    /// Where this Perch's own binary sits. Nowhere any Channel would have put
    /// it, so a test about which Channel installed the machine has to say —
    /// and one that is not about it reads as an Installation Perch did not
    /// make, which is the answer that refuses rather than the one that acts.
    current_exe: RefCell<PathBuf>,
    /// Which user this Perch runs as. `Some(0)` is the one `watcher install`
    /// refuses; the default is an ordinary person's.
    user_id: RefCell<Option<u32>>,
}

/// What is on disk, links included.
///
/// Twelve fields and not split into `Files` and `Links`, because the traffic
/// runs both ways: `files`, `modes`, `dirs`, `modified`, `unwritable` and
/// `undeletable` are all read from the `port::Links` block, and `links` is read
/// from the `port::Files` one. That is the whole reason `port::Filesystem`
/// exists as a supertrait of both (ADR 0056).
struct Filesystem {
    files: RefCell<BTreeMap<PathBuf, String>>,
    /// The permissions of everything that has any, so a test can say that a
    /// Credential was never briefly readable by anyone else.
    modes: RefCell<BTreeMap<PathBuf, u32>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    modified: RefCell<BTreeMap<PathBuf, DateTime<Utc>>>,
    unreadable: RefCell<BTreeMap<PathBuf, String>>,
    /// Paths whose bytes are not text, which the fake cannot hold as one.
    not_text: RefCell<BTreeSet<PathBuf>>,
    unwritable: RefCell<BTreeMap<PathBuf, String>>,
    /// Paths that will not give up what they hold. Distinct from an unwritable
    /// one: a store that refuses a write is routinely one a superseded copy can
    /// still be cleared out of.
    undeletable: RefCell<BTreeMap<PathBuf, String>>,
    /// Directories that are there and will not be walked, while still answering
    /// everything asked about them from the outside.
    ///
    /// Distinct from [`unreadable`], which stops `modified_at` too. A directory
    /// whose own read bit is gone — one a `sudo claude` left root-owned inside a
    /// Profile — is stat-ed from its parent perfectly well and fails `opendir`
    /// with EACCES, and modeled as unreadable it could not be told apart from a
    /// lock artifact whose time will not be read. `lock::clear_the_abandoned`
    /// turns on exactly that difference.
    ///
    /// [`unreadable`]: World::unreadable
    unlistable: RefCell<BTreeMap<PathBuf, String>>,
    /// Files that come back different from how they were written.
    corrupting: RefCell<BTreeSet<PathBuf>>,
    /// Files whose write dies partway, leaving what fitted behind.
    filling: RefCell<BTreeSet<PathBuf>>,
    /// Every link that has been made, by the path that holds it. A hard link is
    /// in here *and* in `files`, because that is what a hard link is: another
    /// name for the file, telling nothing about itself.
    links: RefCell<BTreeMap<PathBuf, (Link, PathBuf)>>,
    /// Whether this Windows can make a symbolic link. Off is the ordinary
    /// machine: the privilege needs Developer Mode or elevation, which is the
    /// whole reason a Run reaches for junctions and hard links (ADR 0026).
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
    /// for minutes without warning.
    ///
    /// Here rather than in [`Stall`] because it says how long *this* surface
    /// stalls, and one concern writes it and reads it.
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
}

/// Time, and what somebody else does while it passes.
///
/// One mechanism with two fields rather than a clock beside a hook. `now` is
/// written at five sites: [`FakeHost::set_now`], and then `while_they_answer`,
/// `keychain_set`, `sleep` and `wait` — and at all four of those the very next
/// statement takes `somebody_else`. Time does not pass in this fake because a
/// clock ticks; it passes because an effect took time, and while that effect was
/// in flight somebody else touched the machine. That is why there is no `Clock`
/// state struct: `impl port::Clock for FakeHost` reads `self.stall.now`
/// (ADR 0059).
struct Stall {
    now: RefCell<DateTime<Utc>>,
    /// What the rest of the machine does the first time Perch waits, and then
    /// does not do again.
    somebody_else: RefCell<Option<WhileWaiting>>,
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

/// Arranging a world, and reading back what Perch did to it.
///
/// The fixture language, and the half of this file that grows fastest: a Host
/// method arrives with the builders a test needs to set it up, so
/// `process_started_at` cost ten lines of trait, thirty-six here, and four
/// `with_*` methods for one concept.
///
/// Kept in the same file as the `impl` blocks below, deliberately. It looks
/// like the obvious split for a file this size and it is not: over the last
/// twenty-five commits that added or removed a function here, eleven touched
/// both halves and eight touched one. Separating them would put the majority of
/// changes across two files and say less about each, not more.
///
/// The same measurement is why the file did not follow the interface when the
/// interface became nine traits (ADR 0056). There are nine `impl` blocks below,
/// one per concern, and one file holding them — and the multiplier this comment
/// measures is untouched by that, because it is per method rather than per
/// trait.
///
/// What was done about it instead is above: the fields these builders arrange
/// are seven concern structs, a [`Stall`] and the recorder rather than forty
/// flat ones, so a builder is grouped against the world it sets up and a reader
/// of terminal code has five fields in scope rather than thirty-nine
/// (ADR 0059). The multiplier is the
/// same — the 43rd method still costs what the 42nd did — because it was never
/// the customer: three of the five fields added over the ten days before that
/// work arrived with no new `Host` method at all. The fake grows from the
/// arrangements tests need, and those arrive whether or not the port widens.
///
/// What does separate cleanly is the *port's* semantics from either — and that
/// is `tests/conformance.rs`, which asks this fake and [`super::RealHost`] the
/// same questions and is where a disagreement between them now shows up.
impl FakeHost {
    pub fn new() -> Self {
        let mut vars = BTreeMap::new();
        vars.insert("USER".to_string(), "someone".to_string());
        FakeHost {
            environment: Environment {
                home: PathBuf::from("/Users/someone"),
                // Spelled out rather than joined onto `home`, because a test
                // that writes this directory into a file writes it the way it
                // is written here. `join` would answer with the separator of
                // whatever platform the tests are running on, and the two
                // spellings would stop matching on Windows.
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
                not_text: RefCell::new(BTreeSet::new()),
                unwritable: RefCell::new(BTreeMap::new()),
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
                // way a real one does. Nothing like the pids the fixtures
                // arrange for other people's clients, so a test can tell whose
                // marker it is looking at.
                live_processes: RefCell::new(BTreeMap::from([(
                    THIS_PROCESS,
                    Some(DateTime::<Utc>::MIN_UTC),
                )])),
            },
            waiting: Waiting {
                listening: RefCell::new(false),
                interrupt_after: RefCell::new(None),
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
            },
            stall: Stall {
                now: RefCell::new(Utc.with_ymd_and_hms(2026, 8, 4, 12, 0, 0).unwrap()),
                somebody_else: RefCell::new(None),
            },
            effects: RefCell::new(Vec::new()),
        }
    }

    // ---- arranging the world -------------------------------------------
    //
    // A builder sits with the struct it arranges, in the order those structs
    // are declared. The names are untouched — `with_file` still says
    // `with_file`, and all 695 call sites in `tests/` say what they always
    // said. What moved is which of them a reader has to hold in their head at
    // once (ADR 0059).

    // ---- environment: the machine it runs on ------------------------

    /// A machine that is not a Mac, where a Credential lives in a file rather
    /// than in a keychain (ADR 0020).
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
    /// Channel installed it (ADR 0039).
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
    /// watcher install` refuses outright (ADR 0040).
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
    pub fn with_unreadable_file(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.fs
            .unreadable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// A directory that is there and will not be walked, while still answering
    /// when it was last written.
    ///
    /// The state a lock left root-owned inside a Profile is in, and the one
    /// `lock::clear_the_abandoned` has to tell from a plain file wedging the
    /// path: `remove_dir_all` and the listing both fail EACCES, and only one of
    /// those is worth ending a command over.
    pub fn with_unlistable_dir(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.fs
            .unlistable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// The same, and its undoing, for a test whose subject is a path becoming
    /// unreadable *while* a command is running — a lock artifact somebody
    /// changes the permissions of mid-hold — rather than one that was so from
    /// the start.
    pub fn set_unreadable(&self, path: impl AsRef<Path>, detail: &str) {
        self.fs
            .unreadable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
    }

    pub fn forget_unreadable(&self, path: impl AsRef<Path>) {
        self.fs.unreadable.borrow_mut().remove(path.as_ref());
    }

    /// A path that cannot be written to, so a test can fail one step of a
    /// multi-step write and see what is left behind.
    pub fn with_unwritable_file(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.fs
            .unwritable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// The same pair, for a path that becomes unwritable partway through a
    /// command rather than before it.
    pub fn set_unwritable(&self, path: impl AsRef<Path>, detail: &str) {
        self.fs
            .unwritable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
    }

    /// Whatever stopped a path being written to has been put right — the
    /// permission fixed, the disk freed — so a test can carry on from a failure
    /// with the world it left rather than a fresh one.
    ///
    /// One name, because there were two with the same three-line body and no
    /// difference between them but the spelling. A reader of a test had to go
    /// and establish that `forget_unwritable` meant this and not something
    /// narrower, which is a question a second name asks and never answers.
    pub fn writable_again(&self, path: impl AsRef<Path>) {
        self.fs.unwritable.borrow_mut().remove(path.as_ref());
    }

    /// A file that is there and will not go — a directory whose permissions
    /// forbid it, a lock some other process holds on Windows. What a Credential
    /// Store that cannot be emptied looks like.
    pub fn with_undeletable_file(self, path: impl AsRef<Path>, detail: &str) -> Self {
        self.fs
            .undeletable
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), detail.to_string());
        self
    }

    /// The same for a path that would not go: what a command that stopped part
    /// way is run again against, once whatever held the path has let go.
    pub fn deletable_again(&self, path: impl AsRef<Path>) {
        self.fs.undeletable.borrow_mut().remove(path.as_ref());
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
    /// bytes that fitted are on disk, and the call fails.
    ///
    /// Distinct from [`with_unwritable_file`], which models the write never
    /// starting. The real host opens the file, then `write_all`s, then
    /// `sync_all`s, so an `ENOSPC` or an `EIO` between the first and the last
    /// leaves a partially-written file behind — and a fake that could only fail
    /// before creating anything let `replace_via_tmp` claim a cleanup it did not
    /// have. Two tests asserted that nothing is left beside the target across a
    /// failed write, and neither could reach the arrangement that leaves one.
    ///
    /// [`with_unwritable_file`]: Self::with_unwritable_file
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

    /// A link that is already there — including one pointing at nothing, which
    /// is what a Profile holds after the entry it shared was deleted and is the
    /// state a Reconcile has to repair.
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

    /// A machine that has a keychain although it is not a Mac.
    ///
    /// The default off macOS is a keychain that answers the way a real one
    /// would there: `/usr/bin/security` is not present on Linux or Windows, so
    /// every call to it fails. A fake keychain that worked everywhere let a
    /// test pass on a scenario that cannot happen on the platform it claims to
    /// be about — so having one off macOS has to be asked for, and asking for
    /// it is a statement that the test is about the composite reader rather
    /// than about that platform.
    pub fn with_keychain_off_macos(self) -> Self {
        *self.keys.keychain_everywhere.borrow_mut() = true;
        self
    }

    /// Every keychain operation now fails as locked or denied, rather than as
    /// "not found" — the distinction ADR 0008 insists on.
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

    /// A keychain that takes a write, reports success, and keeps only the
    /// first `bytes` of what it was given.
    ///
    /// Exactly what `security -i` does when a command line overruns its 4096
    /// byte stdin buffer: it truncates mid-argument and says nothing (ADR
    /// 0008). Perch reads every Credential back before trusting it precisely
    /// because of this, and without a store that can do it the read-back guard
    /// could be deleted with every test still passing.
    pub fn with_keychain_truncating_after(self, bytes: usize) -> Self {
        *self.keys.keychain_keeps.borrow_mut() = Some(bytes);
        self
    }

    /// A keychain that stops to ask the user for permission, and how long they
    /// take to answer it.
    ///
    /// The stall a Switch documents as unbounded: `store_credential` is one
    /// keychain write, and on macOS it can put a dialog in front of somebody who
    /// then walks away. Everything Perch is holding has to survive it.
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
    /// fake is already borrowed and cannot be consumed.
    ///
    /// What a login *changes* is read back by a later command, so a fake where
    /// the two could not disagree could not model a login at all: `perch relogin`
    /// clears a Quarantine, and the `perch list --json` after it has to say
    /// something the one before it did not.
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
    /// wearing a marker that was written before the process began (ADR 0022).
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
    /// written before that instant names somebody else now (ADR 0022).
    pub fn with_this_process_replaced_at(self, at: DateTime<Utc>) -> Self {
        self.processes
            .live_processes
            .borrow_mut()
            .insert(self.process_id(), Some(at));
        self
    }

    // ---- waiting: a loop, and the person who stops it ---------------

    /// A person who presses Ctrl-C after the loop has waited this many times,
    /// which is how many times round it goes.
    ///
    /// Counted in waits rather than in rounds because that is what the loop
    /// actually asks the machine: a test says "three polls and then stop", and
    /// gets exactly the three the person watching would have seen.
    pub fn with_interrupt_after(self, waits: u32) -> Self {
        *self.waiting.interrupt_after.borrow_mut() = Some(waits);
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
    /// passphrase, prompted and confirmed (ADR 0014).
    pub fn with_secrets(self, secrets: &[&str]) -> Self {
        *self.terminal.secrets.borrow_mut() = secrets.iter().map(|line| line.to_string()).collect();
        self
    }

    /// How long the person at the terminal takes to answer each question.
    ///
    /// Instant by default, which no real terminal is. A question put to a human
    /// is the one wait in Perch with no bound on it — somebody may answer in a
    /// second or come back after lunch — and it is the only place a command
    /// behaving perfectly well can outlast a lock it is holding.
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
    /// figure follows while something watches it.
    ///
    /// The last reply answers every call after it, so a trace says how the
    /// figure moves and then how it stays — a test about a threshold being
    /// crossed says the crossing, and does not have to keep saying it for
    /// however many rounds the loop has left.
    pub fn with_replies_to(self, url: &str, bearer: &str, replies: &[(u16, &str)]) -> Self {
        self.network.traces.borrow_mut().insert(
            (url.to_string(), Some(bearer.to_string())),
            replies
                .iter()
                .map(|(status, body)| HttpResponse {
                    status: *status,
                    body: (*body).to_string(),
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
                body: body.to_string(),
            },
        );
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

    /// What the rest of the machine does the first time Perch waits — for a
    /// lock, or for an answer.
    ///
    /// Once, because it stands for a thing that happens rather than for a
    /// condition: a client starting, a lock being given back, another `perch`
    /// arriving.
    pub fn once_while_waiting(self, happens: impl Fn(&FakeHost) + 'static) -> Self {
        *self.stall.somebody_else.borrow_mut() = Some(Box::new(happens));
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
            if dir.as_os_str().is_empty() || self.fs.dirs.borrow().contains(dir) {
                break;
            }
            missing.push(dir.to_path_buf());
            at = dir.parent();
        }

        let mut dirs = self.fs.dirs.borrow_mut();
        let mut modes = self.fs.modes.borrow_mut();
        for dir in missing {
            modes.entry(dir.clone()).or_insert(mode);
            dirs.insert(dir);
        }
    }

    /// What a path names once its links are followed, or `None` when it names
    /// nothing — a link whose target has gone, most of all.
    ///
    /// The directories above it are followed too, which they used to not be on
    /// the reasoning that "a file inside a linked directory is not something
    /// Perch ever asks this fake about". It is exactly what Perch asks about:
    /// the hazard `reconcile::HELD_BACK` holds `sessions` back for is a Profile
    /// whose `sessions` is a link into another one, and reading a marker there
    /// is reading a file inside a linked directory. Without this the whole of
    /// ADR 0027's reason for existing was a state no test could build.
    ///
    /// Bounded on both counts: the walk down a chain of links gives up rather
    /// than hanging on two that point at each other, and the recursion up
    /// through the parents is one level per component of a path that is already
    /// finite.
    fn resolved(&self, path: &Path) -> Option<PathBuf> {
        const FOLLOWED: usize = 8;

        let mut at = path.to_path_buf();
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
                at = target;
                continue;
            }
            // Nothing of that name, which does not mean nothing is there: a
            // directory *above* it may be a link, and the name has to be tried
            // again under whatever that resolves to. A rejoin that changes
            // nothing is a path that was never going to resolve, and stops the
            // walk rather than spending the rest of its attempts on it.
            let rejoined = self.resolved(at.parent()?)?.join(at.file_name()?);
            if rejoined == at {
                return None;
            }
            at = rejoined;
        }
        None
    }

    /// The path a read lands on, or the refusal it meets on the way.
    ///
    /// `RealHost` follows a symbolic link on every read it makes — `read_to_string`,
    /// `metadata`, `read_dir` all do — and this fake followed one on none of them.
    /// It was not even consistent with itself: `path_exists(link)` answered `true`
    /// through [`resolved`](FakeHost::resolved) while `read_file(link)` answered
    /// `NotFound`. So the machines a link makes could not be built here at all: a
    /// `~/.claude.json` managed by stow or chezmoi, and the `sessions` linked into
    /// another Profile that ADR 0027 names.
    ///
    /// A path arranged as unreadable refuses whether it is the link or what the
    /// link points at, because on a real machine either one stops the read.
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
    /// whatever the test said happens while Perch waits happen.
    ///
    /// Shared by both prompts, because a question is a wait whether or not the
    /// terminal shows what is typed — and the passphrase prompts are the longest
    /// wait in Perch, since there are two of them and both are somebody typing
    /// carefully. A fake that passed time at one prompt and not the other would
    /// present a world that stood still for exactly the command most able to
    /// outlast the state it checked.
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

    /// Lets whatever the test said happens while Perch waits happen, once.
    ///
    /// Called from all four sites that move the clock, which is the whole of why
    /// `now` and `somebody_else` are one [`Stall`] rather than a clock beside a
    /// hook (ADR 0059).
    ///
    /// Taken out before it runs, so it can reach back into the fake without
    /// meeting a borrow the call it interrupts is still holding.
    fn somebody_else_arrives(&self) {
        let happens = self.stall.somebody_else.borrow_mut().take();
        if let Some(happens) = happens {
            happens(self);
        }
    }

    fn mark_written(&self, path: &Path) {
        self.fs
            .modified
            .borrow_mut()
            .insert(path.to_path_buf(), *self.stall.now.borrow());
    }

    /// The file a write is *for*: the path itself, or — for the copy written
    /// beside a target by [`super::replace_via_tmp`] — the target it is about
    /// to be renamed over.
    ///
    /// Every arrangement a test makes is about a file it can name. It cannot
    /// name the copy beside one, because that copy carries the pid of whoever
    /// is writing it, and the arrangement is about the disk and the directory
    /// rather than about a filename anyway.
    fn intended(&self, path: &Path) -> PathBuf {
        let suffix = format!(".perch-tmp.{}", self.process_id());
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
}

/// As much of `text` as fits in `bytes`, cut at a character boundary.
///
/// One function, because the fake stands three different things in for by
/// keeping a prefix — a store that corrupts what it holds, a disk that fills
/// partway, and a keychain that will only take so much — and two of them were
/// cutting with `String::truncate` at exactly half the byte length. That panics
/// on any index that is not a character boundary, and what is being halved is
/// registry JSON and `.claude.json`: an Alias, an email address or a project
/// path with one accented letter in it is enough. The fake would then panic
/// inside the arrangement rather than exercise the cleanup it was written for.
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
        // CLAUDE_CONFIG_DIR=` is ordinary shell state, and it reads as unset
        // there. Without this the fake answers `Some("")`, which derives a
        // store whose config directory is `""` and whose plaintext store is
        // `.credentials.json` relative to wherever the process happens to be —
        // a state no real machine can produce, and one a test could be written
        // against in either direction.
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

    fn user_id(&self) -> Option<u32> {
        *self.environment.user_id.borrow()
    }
}

impl port::Files for FakeHost {
    fn read_file(&self, path: &Path) -> Result<String, HostError> {
        self.record(Effect::ReadFile(path.to_path_buf()));
        let at = self.through_links(path)?;
        // The real read is `read_to_string`, so bytes that are not UTF-8 come
        // back as `InvalidData` rather than as contents. The fake holds files
        // as `String` and could not otherwise reach that answer at all.
        if self.fs.not_text.borrow().contains(&at) {
            return Err(HostError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
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
        // same path unless it is the copy written beside a target. A test
        // arranging a full disk or a truncating store names the file it cares
        // about, and the real machine answers the same way for the copy beside
        // it: they are the same directory, and it is the same disk.
        let intended = self.intended(path);
        if let Some(detail) = self.fs.unwritable.borrow().get(&intended) {
            return Err(HostError::Other(detail.clone()));
        }
        self.note_directories_of(path);
        // A disk that fills partway leaves what fitted behind and then fails,
        // which is the order the real host does it in: open, `write_all`,
        // `sync_all`. A fake that could only refuse before creating anything
        // could not model it, so the cleanup on this path went untested.
        if self.fs.filling.borrow().contains(&intended) {
            self.fs.files.borrow_mut().insert(
                path.to_path_buf(),
                a_prefix_of(contents, contents.len() / 2),
            );
            self.fs.modes.borrow_mut().insert(path.to_path_buf(), mode);
            self.mark_written(path);
            return Err(HostError::Other(
                "No space left on device (os error 28)".to_string(),
            ));
        }
        self.fs
            .files
            .borrow_mut()
            .insert(path.to_path_buf(), self.as_stored(&intended, contents));
        self.fs.modes.borrow_mut().insert(path.to_path_buf(), mode);
        self.mark_written(path);
        Ok(())
    }

    /// Written beside and moved into place, exactly as the real one is.
    ///
    /// Through [`super::replace_via_tmp`] rather than straight into the map,
    /// because what this call promises is not only "the bytes and the mode" but
    /// "and never a half-written file at the path". A fake that wrote directly
    /// could not fail the way the real one fails — the real host's `ENOSPC`
    /// lands on the copy *beside* the target and leaves the target untouched,
    /// while this one used to fail at the target itself — so
    /// `a_save_that_fails_leaves_the_registry_exactly_as_it_was` asserted the
    /// absence of a temp file the fake could never have created, and rewriting
    /// the real one as a plain truncate-and-write would have left the suite
    /// green. For the file the registry module calls the whole of Perch's
    /// state.
    ///
    /// The mode is still recorded, which is the other thing this call promises:
    /// "created private" and "made private afterwards" are the distinction ADR
    /// 0020 turns on, and `create_file_with_mode` underneath keeps it.
    fn write_private_file(&self, path: &Path, contents: &str) -> Result<(), HostError> {
        self.record(Effect::WrotePrivateFile(path.to_path_buf()));
        if let Some(parent) = path.parent() {
            self.make_dirs(parent, PRIVATE_DIR_MODE);
        }
        super::replace_via_tmp(self, path, contents, PRIVATE_FILE_MODE)
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
        // Through a link, as `metadata` reads through one: the mode of a
        // symbolic link is not a thing anybody asks about.
        let Some(path) = self.resolved(path).as_deref().map(Path::to_path_buf) else {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        };
        let path = path.as_path();
        // Windows has no mode and relies on the profile ACL (ADR 0020), and the
        // real Host answers `None` there. A fake that answered with a number
        // would let a test drive `tighten_if_loose` — reading the mode, making
        // the file private, remarking that others could read it — on a platform
        // where none of that happens, which is a test asserting behavior the
        // real Host cannot produce.
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

    /// Through a link, as `set_permissions` narrows what one points at.
    fn make_private(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::MadePrivate(path.to_path_buf()));
        let Some(path) = self.resolved(path) else {
            return Err(HostError::NotFound {
                path: path.to_path_buf(),
            });
        };
        let path = path.as_path();
        // A file whose *permissions* cannot be changed is the same arrangement
        // as one whose contents cannot: a `chmod` on a file owned by somebody
        // else fails with `EPERM` however readable it is. Without this the fake
        // had no way to be a machine where tightening a loose Credential does
        // not work, which is the branch that decides whether the user is ever
        // told about a world-readable refresh token.
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
        self.make_dirs(path, ORDINARY_DIR_MODE);
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
        // A link at the path is taken away and what it points at is left
        // alone: `remove_dir_all` does not follow the last component, so it
        // unlinks the link itself and answers `Ok`. Measured, because the
        // opposite belief made `lock::clear_the_abandoned` unreachable in every
        // test — a dangling artifact was modeled as wedging Perch for ever
        // where the machine recovers on the next command.
        if self.fs.links.borrow_mut().remove(path).is_some() {
            self.fs.modified.borrow_mut().remove(path);
            self.fs.files.borrow_mut().remove(path);
            self.fs.modes.borrow_mut().remove(path);
            return Ok(());
        }
        // A plain file is not a directory, and there the real call does say so
        // rather than removing it. A fake that removed it anyway would recover
        // from a state a real machine cannot: see the note on
        // `create_dir_exclusive` below, which is what puts Perch there.
        if self.fs.files.borrow().contains_key(path) {
            return Err(HostError::Other(format!(
                "{}: Not a directory (os error 20)",
                path.display()
            )));
        }
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
        Ok(())
    }

    fn create_dir_exclusive(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::Took(path.to_path_buf()));
        // Told apart from the path being taken. `AlreadyExists` is contention
        // and anything else is the filesystem refusing, and `lock::take` answers
        // them differently — one is waited out and the other is reported — so a
        // fake that could only produce the first left the second untested.
        if let Some(detail) = self.fs.unwritable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // A link in the way counts, whether or not it still resolves.
        //
        // `path_exists` follows a link and answers `false` for a broken one,
        // which is right for the question a Reconcile asks and wrong for this
        // one: `mkdir` does not follow the last component, so it fails
        // `EEXIST` at a symlink whatever the symlink points at. A fake that
        // created the directory *over* a dangling link could not represent the
        // state where a Profile holds a link into a Default Profile that has
        // gone — and in that state a real `lock::take` gets `AlreadyExists`,
        // reads the lock as abandoned because the link will not say when it was
        // written, tries `remove_dir_all` on a symlink and is ignored, and
        // refuses every Switch and every Run against that Profile for ever.
        if self.fs.links.borrow().contains_key(path) || self.path_exists(path) {
            return Err(HostError::AlreadyExists {
                path: path.to_path_buf(),
            });
        }
        self.fs.dirs.borrow_mut().insert(path.to_path_buf());
        self.mark_written(path);
        Ok(())
    }

    /// A path arranged as unreadable will not say when it was written either —
    /// which is how "the lock is gone" and "the lock will not say" are told
    /// apart, and they are different answers.
    fn modified_at(&self, path: &Path) -> Result<DateTime<Utc>, HostError> {
        if let Some(detail) = self.fs.unreadable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        // Not through a link. `metadata` follows one, but a dangling link is
        // exactly the state `lock::abandoned` has to be able to meet: the real
        // call answers `NotFound` there, and so does this — which is the arm
        // that reads a lock as free. A link that does resolve is followed.
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
        // `Io`, which is what the real host answers: `rename_replacing`
        // propagates the `ENOENT` rather than naming it, and `NotFound` is
        // load-bearing elsewhere — `CredentialStore::read` reads it as "this
        // store holds nothing" and `clients_in` as "nothing is running". A fake
        // that answers it here is the answer the next caller to match on a
        // failed rename would be written against.
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
        // And it replaces whatever was at the target, a link included:
        // `rename(2)` does not follow the last component, so a symlink there is
        // unlinked rather than written through. Left behind, the fake reported
        // one path as both a regular file and a link — `read_file` answering
        // with the new contents while `link_target` went on naming somewhere
        // else — and that is precisely the state `replace_via_tmp` exists to
        // produce: a private write onto a planted link is the *security*
        // property, and a test asserting it would have asserted the opposite of
        // what the machine does and still passed.
        self.fs.links.borrow_mut().remove(to);
        // The directories a rename's target sits in are there by the time it
        // lands, because the write that made the temporary file beside it made
        // them. Recorded here too, so a `rename` reached any other way leaves
        // the fake describing a filesystem that could exist.
        self.note_directories_of(to);
        self.mark_written(to);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedFile(path.to_path_buf()));
        if let Some(detail) = self.fs.undeletable.borrow().get(path) {
            return Err(HostError::Other(detail.clone()));
        }
        self.fs.files.borrow_mut().remove(path);
        self.fs.links.borrow_mut().remove(path);
        self.fs.modified.borrow_mut().remove(path);
        self.fs.modes.borrow_mut().remove(path);
        Ok(())
    }

    /// Through a link, as `read_dir` reads through one — but reporting what was
    /// found under the name that was asked about, which is what `read_dir` does
    /// with the path it was given. A `sessions` linked into another Profile
    /// therefore answers with that Profile's markers, which is the whole of the
    /// hazard ADR 0027 names.
    fn list_dir(&self, asked: &Path) -> Result<Vec<PathBuf>, HostError> {
        let resolved = self.resolved(asked);
        let Some(path) = resolved
            .clone()
            .filter(|at| self.fs.dirs.borrow().contains(at))
        else {
            // Something of that name that is not a directory is `ENOTDIR`, not
            // `ENOENT`, and the two are opposite answers to the caller that
            // matters: `probe::clients_in` reads `NotFound` as "no client has
            // ever run here, so nothing is running" and lets a Switch replace
            // the live Credential, and anything else as doubt it refuses on. So
            // a `<profile>/sessions` that is a regular file — a botched restore,
            // a name crossed by a hard link — read as idle in every behavior
            // test and as a refusal on the machine.
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
    /// Makes a link, refusing the kinds this machine could not make.
    ///
    /// The refusals are the point of having this behind the port at all: a
    /// symbolic link on a Windows without Developer Mode, and a junction
    /// anywhere else, fail here exactly as they would there — so the fallbacks
    /// ADR 0026 turns on are exercised from whatever machine the tests run on.
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

        let windows = self.platform() == Platform::Windows;
        match kind {
            Link::Symbolic if windows && !*self.fs.developer_mode.borrow() => {
                return Err(HostError::Other(
                    "A required privilege is not held by the client. (os error 1314)".to_string(),
                ));
            }
            Link::Junction if !windows => {
                return Err(HostError::Other(
                    "a directory junction is a Windows link, and this is not Windows".to_string(),
                ));
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
        // Only a hard link, which is a real second name for the file and shares
        // its modification time. A symbolic link has none of its own: the real
        // `modified_at` goes through `metadata`, which follows the link — so a
        // dangling one is `NotFound`, and that is the arm `lock::abandoned`
        // reads as a lock nobody holds. Marking the link path here gave it a
        // time of its own and answered `Ok`, which is the opposite arm.
        if kind == Link::Hard {
            self.mark_written(at);
        }
        Ok(())
    }

    fn link_target(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
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
        // A hard link is a name for the file, so removing it removes that name
        // — and only that name.
        if self.fs.links.borrow_mut().remove(path).is_some() {
            self.fs.files.borrow_mut().remove(path);
            self.fs.modes.borrow_mut().remove(path);
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
        if let Some(error) = self.lock_error() {
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
        let asked = *self.keys.keychain_set_takes_millis.borrow();
        if asked > 0 {
            let answered = *self.stall.now.borrow() + chrono::Duration::milliseconds(asked as i64);
            *self.stall.now.borrow_mut() = answered;

            // A wait like any other, so what the test said happens while Perch
            // waits happens here too — another `perch` arriving is the whole
            // point of a stall this long.
            self.somebody_else_arrives();
        }
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
        if let Some(error) = self.lock_error() {
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
    /// the program a Run launched: it writes whatever the configured [`Login`]
    /// leaves in the directory it was pointed at, and exits as that says.
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
        // The identifiers the real host refuses before it asks, refused here
        // too. `kill` reads `0` as the caller's whole process group and `-1` as
        // every process it may signal, so neither is a question about one
        // process — and `clients_in` parses a pid out of any filename in a
        // sessions directory, which is not a name Perch wrote. A fake that
        // answered "alive" where the real one says "dead" is a fake a test
        // could prove the wrong behavior against.
        if pid == 0 || pid == u32::MAX {
            return false;
        }
        self.processes.live_processes.borrow().contains_key(&pid)
    }

    fn process_started_at(&self, pid: u32) -> Option<DateTime<Utc>> {
        self.processes
            .live_processes
            .borrow()
            .get(&pid)
            .copied()
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

    /// Passes the time the same way a sleep does, and ends the way the test
    /// said the person watching would end it.
    fn wait(&self, millis: u64) -> Waited {
        self.record(Effect::Waited { millis });

        // Decided before the clock moves, because the real one decides before
        // it sleeps: `RealHost::wait` asks `interrupted()` ahead of the first
        // slice and returns having spent nothing. Advancing first meant the
        // wait that *ends* an interrupted `perch watcher run` spent its whole
        // interval — 2.5 minutes, or 20 under back-off — that a real Ctrl-C
        // never spends, so every "as of 4m ago" measured after one was
        // measuring a duration production does not have.
        //
        // Nothing is interrupted where nothing is listening, for the same
        // reason as on a real machine: Ctrl-C ends the process instead, and a
        // process that has ended waits no more.
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

        // Run either way, and this is the one site where that is a decision: it
        // is the test's scripted event, and what it stands for — another `perch`
        // arriving — does not stop happening because this one was interrupted.
        // So an interrupted wait is the one place the clock stands still and
        // somebody else arrives anyway.
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

    fn note(&self, line: &str) {
        let mut notes = self.terminal.notes.borrow_mut();
        if !notes.iter().any(|said| said == line) {
            notes.push(line.to_string());
        }
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        self.record(Effect::Asked);
        self.while_they_answer();
        Ok(self.terminal.answers.borrow_mut().pop_front())
    }

    fn read_secret(&self) -> Result<Option<String>, HostError> {
        self.record(Effect::AskedInSecret);
        self.while_they_answer();
        Ok(self.terminal.secrets.borrow_mut().pop_front())
    }
}

impl port::Network for FakeHost {
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
            within_millis: request.within_millis,
        };
        self.record(Effect::Http {
            url: sent.url.clone(),
        });
        let asked = sent.url.clone();
        let bearer = sent.bearer().map(str::to_string);
        self.network.sent.borrow_mut().push(sent);

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

    /// `mkdir` does not follow the last component of the path it is given, so
    /// it fails `EEXIST` at a symlink whatever that symlink points at. The fake
    /// answered this through `path_exists`, which *does* follow — and answers
    /// `false` for a broken link — so it created the directory straight over
    /// one.
    ///
    /// The state that reaches is a Profile holding a link into a Default
    /// Profile that has gone, which `reconcile::sweep` exists to prevent and
    /// which a machine can still arrive at. There, `lock::take` gets
    /// `AlreadyExists`, reads the lock as abandoned because a dangling link
    /// will not say when it was written, calls `remove_dir_all` on a symlink
    /// and has the failure ignored — and then refuses every Switch and every
    /// Run against that Profile for ever. No behavior test could reach it.
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

    /// `rename(2)` does not follow the last component either: a symlink at the
    /// target is unlinked and replaced, not written through. That is the whole
    /// of why `replace_via_tmp` is the write a Credential goes through — a link
    /// planted at `.credentials.json` is *replaced* rather than followed to
    /// wherever whoever planted it wanted the secret to land.
    ///
    /// The fake moved the file and left the link entry behind, so afterwards it
    /// answered as both: `read_file` with the new contents, `link_target` with
    /// somewhere else entirely. A test asserting the security property would
    /// have read the link that is not there any more and concluded the opposite
    /// of what the machine does — and passed.
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
    /// `a_process_id_that_is_not_one_is_dead_rather_than_a_process_group`.
    ///
    /// `clients_in` parses a pid out of any filename it finds in a sessions
    /// directory, and those are not names Perch wrote — so `0.json` is
    /// reachable. Unguarded, the fake called that process alive where the real
    /// host calls it dead, which is a fake a test could prove the wrong
    /// behavior against.
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
