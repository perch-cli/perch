//! A Host that keeps the world in memory and records what it was asked to do.
//!
//! Behaviour tests drive real command code against this and assert on
//! observable outcomes: what was printed, what ended up in the keychain, and —
//! for `status` — that no HTTP request was ever attempted (ADR 0015).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};

use super::{Execution, Host, HostError, HttpResponse};
use crate::keychain::KeychainError;

/// One effect the fake was asked to perform, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    ReadFile(PathBuf),
    WroteFile(PathBuf),
    CreatedDir(PathBuf),
    RemovedDir(PathBuf),
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
    HttpGet {
        url: String,
    },
}

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

pub struct FakeHost {
    home: PathBuf,
    now: RefCell<DateTime<Utc>>,
    env: RefCell<BTreeMap<String, String>>,
    files: RefCell<BTreeMap<PathBuf, String>>,
    unreadable: RefCell<BTreeMap<PathBuf, String>>,
    unwritable: RefCell<BTreeMap<PathBuf, String>>,
    dirs: RefCell<BTreeSet<PathBuf>>,
    keychain: RefCell<BTreeMap<(String, String), String>>,
    keychain_lock: RefCell<Option<KeychainLock>>,
    executions: RefCell<BTreeMap<String, Execution>>,
    login: RefCell<Option<Login>>,
    live_processes: RefCell<BTreeSet<u32>>,
    interactive: RefCell<bool>,
    answers: RefCell<VecDeque<String>>,
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
            env: RefCell::new(env),
            files: RefCell::new(BTreeMap::new()),
            unreadable: RefCell::new(BTreeMap::new()),
            unwritable: RefCell::new(BTreeMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            keychain: RefCell::new(BTreeMap::new()),
            keychain_lock: RefCell::new(None),
            executions: RefCell::new(BTreeMap::new()),
            login: RefCell::new(None),
            live_processes: RefCell::new(BTreeSet::new()),
            interactive: RefCell::new(true),
            answers: RefCell::new(VecDeque::new()),
            effects: RefCell::new(Vec::new()),
        }
    }

    // ---- arranging the world -------------------------------------------

    pub fn with_now(self, now: DateTime<Utc>) -> Self {
        *self.now.borrow_mut() = now;
        self
    }

    pub fn with_file(self, path: impl AsRef<Path>, contents: &str) -> Self {
        self.set_file(path, contents);
        self
    }

    /// The same, for a world being arranged from inside a [`Login`], where the
    /// fake is already borrowed and cannot be consumed.
    pub fn set_file(&self, path: impl AsRef<Path>, contents: &str) {
        self.files
            .borrow_mut()
            .insert(path.as_ref().to_path_buf(), contents.to_string());
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

    /// Every keychain operation now fails as locked or denied, rather than as
    /// "not found" — the distinction ADR 0008 insists on.
    pub fn with_locked_keychain(self, detail: &str) -> Self {
        *self.keychain_lock.borrow_mut() = Some(KeychainLock {
            detail: detail.to_string(),
        });
        self
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

    // ---- inspecting what happened --------------------------------------

    pub fn effects(&self) -> Vec<Effect> {
        self.effects.borrow().clone()
    }

    pub fn http_calls(&self) -> Vec<String> {
        self.effects
            .borrow()
            .iter()
            .filter_map(|effect| match effect {
                Effect::HttpGet { url } => Some(url.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn file(&self, path: impl AsRef<Path>) -> Option<String> {
        self.files.borrow().get(path.as_ref()).cloned()
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

    fn record(&self, effect: Effect) {
        self.effects.borrow_mut().push(effect);
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

    fn home_dir(&self) -> PathBuf {
        self.home.clone()
    }

    fn env_var(&self, key: &str) -> Option<String> {
        self.env.borrow().get(key).cloned()
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
        if let Some(parent) = path.parent() {
            self.dirs.borrow_mut().insert(parent.to_path_buf());
        }
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::CreatedDir(path.to_path_buf()));
        self.dirs.borrow_mut().insert(path.to_path_buf());
        Ok(())
    }

    fn path_exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
    }

    fn remove_dir_all(&self, path: &Path) -> Result<(), HostError> {
        self.record(Effect::RemovedDir(path.to_path_buf()));
        self.dirs.borrow_mut().retain(|dir| !dir.starts_with(path));
        self.files
            .borrow_mut()
            .retain(|file, _| !file.starts_with(path));
        Ok(())
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
        self.live_processes.borrow().contains(&pid)
    }

    fn is_interactive(&self) -> bool {
        *self.interactive.borrow()
    }

    fn read_line(&self) -> Result<Option<String>, HostError> {
        Ok(self.answers.borrow_mut().pop_front())
    }

    /// Nothing in this ticket may fetch, so the fake has no canned replies to
    /// give: a request that reaches here is itself the failure, and is recorded
    /// either way for `http_calls` to report.
    fn http_get(&self, url: &str, _headers: &[(&str, &str)]) -> Result<HttpResponse, HostError> {
        self.record(Effect::HttpGet {
            url: url.to_string(),
        });
        Err(HostError::Other(format!(
            "the fake Host has no network: {url}"
        )))
    }
}
