//! The Dogfood harness: what a real machine is asked before a Dogfood run acts
//! on it, and what it is told afterwards (ADR 0037).
//!
//! Every other suite buys determinism by replacing something. This one replaces
//! nothing, so the price is paid here instead: the machine in front of it is
//! asked what it can prove, and only that is proved. What lives in this module
//! is the part of that which is decidable without a real machine — where the
//! marker is, what a Preflight says, which phases a Preflight leaves provable,
//! and how a run is written down. [`phases`] is the list itself; `tests/dogfood.rs`
//! is a driver thin enough to read in one screen.
//!
//! Held back by the `dogfood` feature, for the reason the fakes are: none of it
//! belongs in the binary somebody downloads.
//!
//! Every run opens with a [`repair`], before any phase acts: there are more
//! machines than Accounts, so a Quarantine yesterday's run elsewhere caused is
//! the ordinary starting state rather than an ambush. It cannot fail a run — all
//! it changes is what the machine can prove afterwards.

pub mod phases;

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{PerchError, Result};
use crate::host::{Execution, Host, HttpRequest, Platform};
use crate::registry;

/// The marker file, beside the registry.
///
/// In Perch's own directory rather than somewhere of Dogfood's own, so that
/// `PERCH_HOME` moves the two together: a scratch home is an unmarked machine,
/// which is exactly the answer wanted there. A `perch purge` takes the marker
/// with it, and that is right too — the Export the marker stands for described
/// a machine that no longer exists.
pub const MARKER_FILE: &str = "dogfood.json";

/// The version this build writes, and the only one there has ever been.
pub const MARKER_VERSION: u32 = 1;

/// What the setup wizard writes once it has taken an Export, and what the suite
/// refuses to run without.
///
/// The guarantee is the Export, not the file: *I am fairly sure I ran setup* is
/// exactly the belief that is wrong on the occasion it matters, so the marker is
/// the wizard's receipt rather than a flag anybody could set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub version: u32,
    /// When the wizard ran.
    pub marked_at: DateTime<Utc>,
    /// The Perch the wizard ran as, so a report says which build marked the
    /// machine rather than only which one tested it.
    pub perch_version: String,
    /// The Export the wizard took, or why there was none to take.
    pub export: Held,
}

/// What the wizard's first act came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Held {
    /// An Export was written here, and every Account on this machine is in it.
    Exported { path: PathBuf },
    /// Perch held no Accounts, so there was nothing an Export could have saved.
    /// The only machine this is honest on is one that has never logged in — a
    /// CI runner, or a laptop somebody has just cloned the repository onto.
    NothingHeld,
}

impl Held {
    /// The line a report and the Preflight both say it with.
    pub fn said(&self) -> String {
        match self {
            Held::Exported { path } => format!("Export at {}", path.display()),
            Held::NothingHeld => "no Export: this machine held no Accounts".to_string(),
        }
    }
}

/// How to mark a machine, said in every refusal that is about not having done
/// it. One string, because the two places that print it must not come to
/// disagree about the command.
pub const HOW_TO_SET_UP: &str = "`cargo run --features dogfood --bin dogfood-setup` \
     takes an Export first, and marks the machine after.";

pub fn marker_path(host: &dyn Host) -> Result<PathBuf> {
    Ok(registry::perch_home(host)?.join(MARKER_FILE))
}

/// The marker, or a refusal that says how to make one.
///
/// The suite's first act, before the Preflight and long before a phase: a
/// machine somebody only meant to connect to should not be able to start
/// Switching Accounts around because a command was recalled from history.
pub fn marker(host: &dyn Host) -> Result<Marker> {
    let path = marker_path(host)?;
    let Ok(contents) = host.read_file(&path) else {
        return Err(PerchError::NotFound(format!(
            "This machine has not been set up for a Dogfood run, so the suite \
             will not touch it.\n{HOW_TO_SET_UP}"
        )));
    };

    // Before the parse, for the reason `claimed_version` is written down at.
    if let Some(version) = crate::error::claimed_version(&contents)
        && version > MARKER_VERSION
    {
        return Err(crate::error::written_by_a_newer_perch(
            &path.display().to_string(),
            "dogfood marker",
            version,
            MARKER_VERSION,
        ));
    }

    serde_json::from_str(&contents).map_err(|err| PerchError::Malformed {
        path: path.display().to_string(),
        detail: err.to_string(),
    })
}

/// Writes the marker, which is the last thing the wizard does: everything it
/// stands for has to have happened before it exists.
pub fn mark(host: &dyn Host, marker: &Marker) -> Result<()> {
    let path = marker_path(host)?;
    let home = registry::perch_home(host)?;
    host.create_private_dir_all(&home)
        .map_err(|err| PerchError::file_write(home, err))?;

    let document =
        serde_json::to_string_pretty(marker).map_err(|err| PerchError::Other(err.to_string()))?;
    crate::host::write_atomically(host, &path, &format!("{document}\n"))
        .map_err(|err| PerchError::file_write(path, err))
}

// ---- what this machine holds ----------------------------------------------

/// Which Claude Code is on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Client {
    /// Installed, and reporting this version.
    Installed(String),
    /// Not here, or not something Perch recognises, said as the refusal that
    /// came back.
    Absent(String),
}

/// Whether Anthropic answered at all.
///
/// Asked unauthenticated, so it spends no part of an Account's hourly allowance
/// (ADR 0015) — a 401 is a perfectly good yes. What it rules out is the failure
/// that would otherwise be misread as a defect: a phase that could not renew
/// because nothing on this machine could reach the internet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answering {
    /// Something answered, with this status.
    Yes(u16),
    /// Nothing did, and this is what the attempt said.
    No(String),
}

/// What a Dogfood run establishes about the machine before it acts.
///
/// Said first and said as a figure, because a run that quietly proved a third
/// of what it was asked to and a run that proved all of it look identical once
/// they are over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub platform: Platform,
    /// Windows Subsystem for Linux, where a Profile may sit on `/mnt/c` and
    /// Reconcile has a refusal of its own to make.
    pub wsl: bool,
    pub client: Client,
    pub network: Answering,
    /// Every Account Perch holds, by email address.
    ///
    /// Read from the registry directly rather than through the binary under
    /// test: the Preflight has to be able to describe the machine even when the
    /// thing being tested is the part that is broken.
    pub accounts: Vec<String>,
    /// Which of them are Quarantined. Ordinary rather than alarming — more
    /// machines than Accounts means yesterday's run elsewhere may have retired
    /// what this one holds — which is why Repair is phase zero (ADR 0037).
    ///
    /// Not counted as held for the purposes of [`Needs::accounts`]: a retired
    /// token is not something a phase can prove anything with, and a machine
    /// that can prove nothing reporting that it can prove everything is the
    /// failure the figure was added to prevent.
    pub quarantined: Vec<String>,
    /// The Account that is active, if one is.
    ///
    /// Its own question rather than one a phase discovers: Perch can hold
    /// Accounts with none of them active, and `perch status` is a refusal there
    /// rather than a document. A phase reading one has to be skipped by the
    /// Preflight and counted in the figure, not stopped halfway through.
    pub active: Option<String>,
}

/// What a phase needs of a machine before it can prove anything.
///
/// Declared beside the phase rather than discovered when it fails, so the
/// Preflight can say how much of the suite this machine can prove *before* any
/// of it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Needs {
    /// How many *usable* Accounts Perch must hold, at least — held, and not
    /// Quarantined.
    ///
    /// Held alone was the wrong count: an Account whose refresh token another
    /// machine's run retired is one a phase would run against and then report
    /// Perch as broken over. The Repair clears what it can before this is
    /// counted, and what it could not clear is what the skip line names.
    pub accounts: usize,
    /// A Claude Code to launch.
    pub client: bool,
    /// A network that answers.
    pub network: bool,
    /// One of those Accounts to be active.
    pub active: bool,
}

impl Needs {
    /// A phase that runs anywhere — argv, a refusal, an exit code.
    pub const NOTHING: Needs = Needs {
        accounts: 0,
        client: false,
        network: false,
        active: false,
    };

    /// A phase that reads what Perch holds, and so needs it to hold something
    /// it can still use.
    pub const AN_ACCOUNT: Needs = Needs {
        accounts: 1,
        ..Needs::NOTHING
    };

    /// A phase that reads `perch status`, which is about the Account somebody
    /// is on rather than about the ones they have.
    pub const THE_ACTIVE_ACCOUNT: Needs = Needs {
        active: true,
        ..Needs::AN_ACCOUNT
    };
}

impl Preflight {
    /// Everything asked of the machine, before anything acts on it.
    pub fn taken(host: &dyn Host) -> Result<Preflight> {
        let mut preflight = Preflight {
            platform: host.platform(),
            wsl: is_wsl(host),
            client: match crate::probe::claude_version(host) {
                Ok(version) => Client::Installed(version),
                Err(why) => Client::Absent(why.to_string()),
            },
            network: anthropic_answers(host),
            accounts: Vec::new(),
            quarantined: Vec::new(),
            active: None,
        };
        preflight.read_the_registry(host)?;
        Ok(preflight)
    }

    /// What the registry says, read again.
    ///
    /// For the wizard, which may have just walked a login: what this machine
    /// holds changes inside one setup and the rest does not. Asked apart so the
    /// client and the network are not consulted twice — one is a process and the
    /// other is a round trip, and neither answers differently a minute later.
    pub fn read_the_registry(&mut self, host: &dyn Host) -> Result<()> {
        let registry = registry::load(host)?.unwrap_or_default();

        self.accounts = registry
            .accounts
            .iter()
            .map(|account| account.email().to_string())
            .collect();
        self.quarantined = registry
            .accounts
            .iter()
            .filter(|account| account.quarantined())
            .map(|account| account.email().to_string())
            .collect();
        self.active = registry
            .active_account()
            .map(|account| account.email().to_string());
        Ok(())
    }

    /// The Accounts a phase could actually prove something with: held here, and
    /// not Quarantined.
    ///
    /// The distinction the Repair exists for. A Quarantined Account is one whose
    /// refresh token something retired — usually this suite, running on another
    /// machine yesterday — so a phase pointed at one proves nothing and reports
    /// Perch as broken while doing it.
    pub fn usable(&self) -> Vec<&String> {
        self.accounts
            .iter()
            .filter(|email| !self.quarantined.contains(email))
            .collect()
    }

    /// How many of these phases this machine can prove, as it stands.
    pub fn provable(&self, phases: &[Phase]) -> usize {
        phases
            .iter()
            .filter(|phase| self.unmet(&phase.needs).is_none())
            .count()
    }

    /// The figure, and the whole reason the Preflight is said out loud.
    pub fn figure(&self, phases: &[Phase]) -> String {
        self.figure_said("", phases)
    }

    /// The same, for the machine the Repair left behind.
    ///
    /// Both are said in a run, because a Repair that changed nothing and a
    /// Repair that changed everything must not read alike — and "now" is the
    /// whole of what tells the two figures apart.
    pub fn figure_after_a_repair(&self, phases: &[Phase]) -> String {
        self.figure_said("now ", phases)
    }

    fn figure_said(&self, now: &str, phases: &[Phase]) -> String {
        format!(
            "This machine can {now}prove {} of {}",
            self.provable(phases),
            counted(phases.len())
        )
    }

    /// The first thing this machine has not got that a phase needs, said as the
    /// line the skip is printed with — or nothing, if it can prove it.
    ///
    /// First rather than all of them: a skip line is read to find out what to
    /// fix next, and a machine missing three things is fixed one at a time.
    pub fn unmet(&self, needs: &Needs) -> Option<String> {
        let usable = self.usable();
        if usable.len() < needs.accounts {
            return Some(self.too_few_usable(usable.len(), needs.accounts));
        }
        if needs.client
            && let Client::Absent(why) = &self.client
        {
            return Some(format!("there is no Claude Code to launch: {why}"));
        }
        if needs.network
            && let Answering::No(why) = &self.network
        {
            return Some(format!("Anthropic did not answer: {why}"));
        }
        if needs.active {
            let Some(active) = &self.active else {
                return Some(
                    "no Account is active here, so there is no Account to be about".to_string(),
                );
            };
            // The Account somebody is on can be the Quarantined one, and then
            // `perch status` is about an Account nothing will work as. Its own
            // reason rather than the count above, which this machine may well
            // satisfy with a different Account entirely.
            if self.quarantined.contains(active) {
                return Some(format!(
                    "{active} is the Account this machine is on, and it is still Quarantined"
                ));
            }
        }
        None
    }

    /// Why a phase needing usable Accounts cannot run here.
    ///
    /// Two readings, because one sentence covering both lies in the ordinary
    /// case: with nothing Quarantined, "holds" and "can use" are the same number
    /// and the shorter line is the honest one. With a Quarantine, the difference
    /// between them *is* the answer — a skip line saying Perch holds nothing,
    /// two lines under a Preflight that named two Accounts, sends somebody
    /// looking for a bug in the registry.
    fn too_few_usable(&self, usable: usize, needed: usize) -> String {
        let needed = crate::commands::accounts(needed);
        if self.quarantined.is_empty() {
            return format!(
                "Perch holds {} here and this phase needs {needed}",
                crate::commands::accounts(usable)
            );
        }
        format!(
            "Perch can use {usable} of the {} it holds here and this phase needs \
             {needed} — {} still Quarantined",
            crate::commands::accounts(self.accounts.len()),
            or_none(&self.quarantined)
        )
    }

    /// The word a report file is named after, so a directory of them can be
    /// read as a matrix rather than as a pile of dates.
    pub fn machine(&self) -> &'static str {
        match (self.platform, self.wsl) {
            (_, true) => "wsl",
            (Platform::MacOs, _) => "macos",
            (Platform::Windows, _) => "windows",
            (Platform::Other, _) => "linux",
        }
    }

    /// The Preflight as it is printed and written down: what is here, and then
    /// the figure it all adds up to.
    pub fn said(&self, phases: &[Phase]) -> Vec<String> {
        vec![
            format!("Machine: {}", self.machine()),
            match &self.client {
                Client::Installed(version) => format!("Claude Code: {version}"),
                Client::Absent(why) => format!("Claude Code: none — {why}"),
            },
            match &self.network {
                Answering::Yes(status) => format!("Anthropic answered: HTTP {status}"),
                Answering::No(why) => format!("Anthropic did not answer: {why}"),
            },
            // Named rather than counted, which is what makes a report worth
            // reading a week later — and what makes the directory it goes in
            // one that is never committed.
            format!(
                "Accounts held: {} ({})",
                self.accounts.len(),
                or_none(&self.accounts)
            ),
            // Its own line rather than a parenthesis on the one above: a
            // Quarantine somebody else's run caused is the ordinary starting
            // state, and Repair is phase zero because of it (ADR 0037).
            format!("Quarantined: {}", or_none(&self.quarantined)),
            format!("Active: {}", self.active.as_deref().unwrap_or("none")),
            self.figure(phases),
        ]
    }
}

/// A program that never started, said the same way wherever it is launched from.
///
/// A fault in Perch rather than upstream news, and never anything else: nothing
/// about the machine's Accounts or Anthropic is involved in a binary that could
/// not be executed.
fn could_not_run(bin: &str, args: &[&str], err: &crate::host::HostError) -> Setback {
    Setback::perch(format!("`{bin} {}` could not run: {err}", args.join(" ")))
}

/// A count of Phases, with the noun agreeing with it.
///
/// The same rule [`crate::commands::accounts`] holds for the other noun Perch
/// counts, and here for the same reason: "1 phases" is the kind of thing that
/// ships and stays shipped, and the figure this appears in is the one line the
/// Preflight exists to print.
fn counted(phases: usize) -> String {
    match phases {
        1 => "1 phase".to_string(),
        _ => format!("{phases} phases"),
    }
}

fn or_none(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

/// Whether this is Windows Subsystem for Linux.
///
/// The variables first, because they are what WSL sets for its own sake and are
/// there whether or not `/proc` is mounted the way it usually is; `/proc/version`
/// after, since a shell that scrubbed its environment still leaves that.
fn is_wsl(host: &dyn Host) -> bool {
    if host.env_var("WSL_DISTRO_NAME").is_some() || host.env_var("WSL_INTEROP").is_some() {
        return true;
    }
    host.read_file(Path::new("/proc/version"))
        .is_ok_and(|version| version.to_lowercase().contains("microsoft"))
}

fn anthropic_answers(host: &dyn Host) -> Answering {
    match host.http(&HttpRequest::get(crate::anthropic::PROFILE_URL, &[])) {
        Ok(response) => Answering::Yes(response.status),
        Err(why) => Answering::No(why.to_string()),
    }
}

// ---- the Perch under test --------------------------------------------------

/// Where the binary a run drives comes from, when it is not the one this build
/// produced. Pointing it at an installed Perch is the only way a bug in the
/// release archive itself is ever caught.
pub const BIN_VARIABLE: &str = "PERCH_DOGFOOD_BIN";

/// The Perch a run drives, as a process rather than as a library.
///
/// The whole point of the suite: argv, the exit codes and every rendered line
/// go unasserted anywhere else, because everything else links the library.
pub struct Perch<'a> {
    host: &'a dyn Host,
    bin: PathBuf,
}

impl<'a> Perch<'a> {
    /// `$PERCH_DOGFOOD_BIN`, or the binary this build produced.
    pub fn under_test(host: &'a dyn Host, built: &str) -> Perch<'a> {
        Perch {
            host,
            bin: host
                .env_var(BIN_VARIABLE)
                .map_or_else(|| PathBuf::from(built), PathBuf::from),
        }
    }

    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// Runs `perch` and hands back what it said, whatever it said. A command
    /// that could not be launched at all is a Setback rather than an exit code:
    /// there is no reading of it that is about Perch's behaviour.
    pub fn run(&self, args: &[&str]) -> std::result::Result<Execution, Setback> {
        let bin = self.bin.display().to_string();
        self.host
            .exec(&bin, args)
            .map_err(|err| could_not_run(&bin, args, &err))
    }

    /// Runs `perch` with the terminal attached, and hands back what it exited
    /// with.
    ///
    /// What the wizard walks a login and an Export through: a passphrase is
    /// typed at the terminal and a login is a browser round trip, and neither is
    /// something a captured pipe can stand in for.
    pub fn interactive(&self, args: &[&str]) -> std::result::Result<i32, Setback> {
        let bin = self.bin.display().to_string();
        self.host
            .exec_interactive(&bin, args, &[])
            .map_err(|err| could_not_run(&bin, args, &err))
    }

    /// The same, insisting it succeeded and reading what it printed as JSON.
    ///
    /// The shape every phase reads through: `list --json`, `status --json` and
    /// exit codes are the whole of what a phase may see, and there is no
    /// `perch state --json` (ADR 0037).
    pub fn json(&self, args: &[&str]) -> std::result::Result<serde_json::Value, Setback> {
        let execution = self.run(args)?;
        let said = format!("`perch {}`", args.join(" "));
        if !execution.succeeded() {
            return Err(read_as(execution.status).because(format!(
                "{said} exited {}: {}",
                execution.status,
                execution.stderr.trim()
            )));
        }
        serde_json::from_str(&execution.stdout).map_err(|err| {
            Setback::perch(format!("{said} printed something that is not JSON: {err}"))
        })
    }
}

// ---- a phase, and how one stops -------------------------------------------

/// Which of the two a failure is. A suite that cannot tell them apart is a
/// suite whose red is ignored within a month (ADR 0037).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// A fault in Perch. The run is red and there is a bug to fix.
    Perch,
    /// News about something upstream — Anthropic was slow, a login was ended
    /// elsewhere, the machine was busy. Nothing here is Perch's to fix.
    Upstream,
}

impl Fault {
    /// The sentence that tells the reader who has work to do.
    ///
    /// One string, because a stopped phase and a Repair that could not clear a
    /// Quarantine owe the reader the same answer, and two spellings of it would
    /// eventually come to mean two different things.
    pub fn said(&self) -> &'static str {
        match self {
            Fault::Perch => "This is a fault in Perch.",
            Fault::Upstream => "This is news about something upstream, not a fault in Perch.",
        }
    }

    /// A [`Setback`] of this kind, with nothing changed on the machine. The one
    /// constructor the other two are written in terms of, so which fault a
    /// Setback carries is only ever decided in one place.
    pub fn because(self, because: impl Into<String>) -> Setback {
        Setback {
            fault: self,
            because: because.into(),
            now_true: Vec::new(),
            put_it_back: Vec::new(),
        }
    }
}

/// Which of the two a non-zero exit from a phase's own command is.
///
/// A phase reads through `perch list --json` and `perch status --json`, and the
/// machine it reads on is one somebody works on. Another `perch` holding the
/// registry, a client running against the Profile, a keychain that has locked
/// itself since the Preflight — all of those are the machine being busy, and
/// none of them is a defect. Blaming them on Perch is how a suite's red comes
/// to be ignored (ADR 0037), which is the distinction [`Fault`] exists to draw.
///
/// Everything else is Perch disagreeing with itself: the Preflight named these
/// Accounts through the same binary moments earlier, so a `NotFound` or an
/// `Invalid` now is a bug rather than news.
///
/// Deliberately not [`refused_with`], which reads the same codes for a
/// different question. There, `NotFound` means a browser login the person
/// abandoned and `Conflict` means they signed in as somebody else — news, both
/// of them, because the Repair hands the terminal to a human. A phase hands it
/// to nobody, so the same code means the opposite thing.
fn read_as(status: i32) -> Fault {
    use crate::error::*;
    match status {
        EXIT_HELD | EXIT_PROFILE_LIVE | EXIT_KEYCHAIN_UNAVAILABLE | EXIT_PROBE_REFUSED => {
            Fault::Upstream
        }
        _ => Fault::Perch,
    }
}

/// What a `perch relogin` that did not clear a Quarantine exited with: whose
/// fault it was, and what that code is Perch's way of refusing.
///
/// Read off the exit code, because the Repair hands the terminal to the command
/// and never sees what it printed. The person watching saw the refusal itself
/// go by; what this adds is the same thing in a report read a week later, when
/// a bare `13` says nothing about a browser that was signed in as somebody else.
///
/// It is a reading of the code rather than a second opinion about what happened
/// — the Repair inherits Perch's refusals and does not re-implement them, so
/// nothing here is checked, only named.
///
/// One match for both answers: which of the two a failure is and how it is said
/// are decided by the same fact, and splitting them is how they come to
/// disagree. Each arm is a refusal `relogin` makes deliberately and so is news
/// rather than a defect. Everything else — argv it could not parse, a Cycling
/// code from a command that never Cycles, its own `Quarantined` refusal over the
/// one command that ends a Quarantine — is Perch disagreeing with itself, and
/// that is a bug.
fn refused_with(status: i32) -> (Fault, &'static str) {
    use crate::error::*;
    let news = |said| (Fault::Upstream, said);
    match status {
        EXIT_PROBE_REFUSED => news("Claude Code did not report a version"),
        EXIT_KEYCHAIN_UNAVAILABLE => news("the keychain would not open"),
        EXIT_NOT_FOUND => news("the login did not complete"),
        EXIT_CONFLICT => news("that login came back as a different Account"),
        EXIT_PROFILE_LIVE => news("a client is running against the Profile"),
        EXIT_HELD => news("another Perch is holding the registry"),
        _ => (Fault::Perch, "Perch did not say which refusal that is"),
    }
}

/// A phase that stopped, and everything the person watching needs.
///
/// It never unwinds itself: on a real machine an unwind can fail too, and a
/// failed unwind after a failed assertion leaves a state nobody can read. So it
/// stops, says what is now true, and says what puts it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setback {
    pub fault: Fault,
    pub because: String,
    pub now_true: Vec<String>,
    pub put_it_back: Vec<String>,
}

impl Setback {
    /// A fault in Perch, with nothing changed on the machine.
    pub fn perch(because: impl Into<String>) -> Setback {
        Fault::Perch.because(because)
    }

    /// News about something upstream, with nothing changed on the machine.
    pub fn upstream(because: impl Into<String>) -> Setback {
        Fault::Upstream.because(because)
    }

    /// What the machine looks like now the phase has stopped on it.
    pub fn leaving(mut self, now_true: &[String]) -> Setback {
        self.now_true = now_true.to_vec();
        self
    }

    /// The commands that put it back, as somebody would type them.
    pub fn put_back_with(mut self, commands: &[String]) -> Setback {
        self.put_it_back = commands.to_vec();
        self
    }

    /// The whole of what a stopped phase prints.
    pub fn said(&self) -> String {
        let mut lines = vec![
            self.because.clone(),
            String::new(),
            self.fault.said().to_string(),
            String::new(),
        ];

        if self.now_true.is_empty() {
            lines.push("Nothing on this machine was changed.".to_string());
        } else {
            lines.push("What is true on this machine now:".to_string());
            lines.extend(self.now_true.iter().map(|line| format!("  - {line}")));
        }

        if !self.put_it_back.is_empty() {
            lines.push(String::new());
            lines.extend(to_type("What puts it back:", &self.put_it_back));
        }

        lines.join("\n")
    }
}

/// Commands somebody could type, said the same way wherever a run offers them —
/// a stopped phase's way back, and a Repair's way to finish by hand.
///
/// One place, because a run offers these twice and three spellings of one shape
/// had already grown between two: the heading and the `$` are what somebody
/// scans for, and they should not move depending on which of the two they are
/// reading.
/// What a Repair heads its leftovers with, in one place because the run says it
/// on the terminal and the report says it again on disk.
const FINISH_BY_HAND: &str = "What would finish the job by hand:";

fn to_type(heading: &str, commands: &[String]) -> Vec<String> {
    let mut lines = vec![heading.to_string()];
    lines.extend(commands.iter().map(|command| format!("  $ {command}")));
    lines
}

// ---- phase zero: the Repair ------------------------------------------------

/// How one Quarantined Account the Repair found ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// Perch lists it, and no longer lists it as Quarantined. The login worked.
    Cleared,
    /// It is Quarantined still, for this reason — and whether that is Perch's
    /// to fix or news about something else.
    StillQuarantined { fault: Fault, why: String },
    /// Perch does not list the Account at all any more.
    ///
    /// Its own ending rather than a [`Ending::Cleared`], which is what reading
    /// "is it Quarantined?" off a listing alone would have made it: `relogin`
    /// refuses when the Account was removed while the login was happening, and
    /// a run recording that as a repair would claim to have proved something
    /// about an Account that is gone.
    NoLongerListed,
}

/// One Quarantined Account, and what opening the run did about it.
///
/// Per Account rather than one verdict for the whole Repair: a run against three
/// Accounts where one login was abandoned has to be legible as exactly that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub account: String,
    /// Why it was Quarantined when the Repair found it, as `perch list --json`
    /// gave the reason.
    pub was_quarantined_because: String,
    /// What `perch relogin` exited with, or nothing where it was never launched.
    pub exited: Option<i32>,
    pub ended: Ending,
}

impl Attempt {
    /// What was wrong with this Account, said before the browser opens.
    ///
    /// Somebody about to walk a login is owed what stopped working, and the two
    /// halves are apart because minutes of browser round trip sit between them:
    /// this is said going in, [`Attempt::ended_as`] the moment it comes back.
    pub fn found(&self) -> String {
        Attempt::what_was_wrong(&self.account, &self.was_quarantined_because)
    }

    /// The same line before there is an Attempt to hang it on. The Repair says
    /// what was wrong on the way in and only learns how it ended minutes later,
    /// so the sentence has to exist without the ending.
    fn what_was_wrong(account: &str, was_quarantined_because: &str) -> String {
        format!("{account} was Quarantined: {was_quarantined_because}")
    }

    /// How it ended.
    pub fn ended_as(&self) -> String {
        let ran = match self.exited {
            Some(status) => format!("`perch relogin {}` exited {status}", self.account),
            None => "no login was attempted".to_string(),
        };
        match &self.ended {
            Ending::Cleared => format!("{ran} — it is no longer Quarantined"),
            Ending::StillQuarantined { fault, why } => {
                format!("{ran} — it is Quarantined still: {why}. {}", fault.said())
            }
            Ending::NoLongerListed => format!(
                "{ran} — Perch does not list it at all any more, so nothing was \
                 repaired. {}",
                Fault::Upstream.said()
            ),
        }
    }

    /// Both, for a report read a week later with no run around it.
    pub fn said(&self) -> [String; 2] {
        [self.found(), self.ended_as()]
    }
}

/// What opening the run came to.
///
/// It can never be a [`Setback`]. A Quarantine another machine's run caused is
/// the ordinary starting state rather than a defect (ADR 0037), so a Repair that
/// could not clear one records why and the run carries on. The one thing that
/// changes is what the machine can prove afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repair {
    /// Perch holds nothing here, so there was nothing to repair. A CI runner,
    /// and the reason a degraded run needs no terminal at all.
    NothingHeld,
    /// Everything this machine holds is usable. The ordinary case, and one line.
    NothingQuarantined,
    /// The listing could not be read, so the Repair never learned what was
    /// Quarantined. Recorded rather than raised: failing over a `perch list`
    /// that does not work is a phase's job, not phase zero's.
    NotRead(String),
    /// The Quarantined Accounts, in the order the listing gave them, and what
    /// came of each.
    Found(Vec<Attempt>),
}

impl Repair {
    /// The whole of what a Repair is written down as.
    pub fn said(&self) -> Vec<String> {
        match self {
            Repair::NothingHeld => {
                vec!["Perch holds no Accounts here, so there was nothing to repair.".to_string()]
            }
            Repair::NothingQuarantined => {
                vec!["Nothing is Quarantined here, so there was nothing to repair.".to_string()]
            }
            Repair::NotRead(why) => vec![format!(
                "Nothing could be repaired: {why}. {}",
                Fault::Perch.said()
            )],
            Repair::Found(attempts) => attempts.iter().flat_map(Attempt::said).collect(),
        }
    }

    /// Every Account the Repair left Quarantined.
    pub fn left_quarantined(&self) -> Vec<&str> {
        match self {
            Repair::Found(attempts) => attempts
                .iter()
                .filter(|attempt| matches!(attempt.ended, Ending::StillQuarantined { .. }))
                .map(|attempt| attempt.account.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The commands that would finish the job by hand.
    ///
    /// Nothing is unwound, here as everywhere else in the suite: a Repair that
    /// got halfway leaves the machine where it is, says what is true, and says
    /// what somebody could type to put the rest right themselves.
    pub fn finish_by_hand(&self) -> Vec<String> {
        // A Repair that could not read the listing does not know which Accounts
        // to name, so it offers the command that would say — leaving somebody
        // with nothing to type is the one outcome that helps nobody.
        if let Repair::NotRead(_) = self {
            return vec!["perch list --json".to_string()];
        }
        self.left_quarantined()
            .iter()
            .map(|account| format!("perch relogin {account}"))
            .collect()
    }

    /// Whether it said itself as it went, one Account at a time.
    ///
    /// Only a Repair that found Quarantined Accounts has anything to interleave:
    /// somebody three browser round trips into a run cannot wait until the end
    /// to find out which of them failed. The rest are a line, and are said once
    /// the answer is known.
    fn said_itself(&self) -> bool {
        matches!(self, Repair::Found(_))
    }
}

/// Phase zero: the Quarantines another machine's run left behind, cleared before
/// any phase acts (ADR 0037).
///
/// It reads `list --json` rather than the registry, unlike the Preflight: the
/// Repair acts *through* the binary under test, so it should see what that
/// binary sees. It spends no Utilization — a listing renders from cache and a
/// browser login is not an API call — and it cannot stop the run.
///
/// The terminal goes to `perch relogin`, one Account at a time. Every refusal
/// that command already makes is inherited rather than pre-empted: it refuses
/// while a client is running against the Profile, it requires a Claude Code that
/// reports a version, and it refuses a login that came back as somebody else.
/// Two implementations of one rule is how they come to disagree.
pub fn repair(perch: &Perch<'_>, preflight: &Preflight, out: &mut dyn Write) -> Result<Repair> {
    say(out, "")?;
    say(out, "Repair")?;

    let repair = what_it_came_to(perch, preflight, out)?;

    if !repair.said_itself() {
        for line in repair.said() {
            say(out, &format!("  {line}"))?;
        }
    }

    let by_hand = repair.finish_by_hand();
    if !by_hand.is_empty() {
        say(out, "")?;
        for line in to_type(FINISH_BY_HAND, &by_hand) {
            say(out, &format!("  {line}"))?;
        }
    }

    Ok(repair)
}

fn what_it_came_to(
    perch: &Perch<'_>,
    preflight: &Preflight,
    out: &mut dyn Write,
) -> Result<Repair> {
    // Asked of the Preflight rather than of the binary, and it is the one thing
    // that is: `perch list` refuses on a machine holding nothing, so a Repair
    // that opened with it would report a machine with nothing to repair as a
    // machine it could not read. It is also what keeps a CI runner from needing
    // a terminal.
    if preflight.accounts.is_empty() {
        return Ok(Repair::NothingHeld);
    }

    let listed = match listed_now(perch) {
        Ok(listed) => listed,
        Err(setback) => return Ok(Repair::NotRead(setback.because)),
    };
    let quarantined: Vec<(String, String)> = listed
        .into_iter()
        .filter_map(|(email, why)| Some((email, why?)))
        .collect();
    if quarantined.is_empty() {
        return Ok(Repair::NothingQuarantined);
    }

    let mut attempts = Vec::new();
    for (account, was_quarantined_because) in quarantined {
        // A line between each, because three logins in a row are three browser
        // round trips and somebody has to be able to tell which one they are on.
        say(out, "")?;
        let attempt = repair_one(
            perch,
            &preflight.client,
            account,
            was_quarantined_because,
            out,
        )?;
        say(out, &format!("  {}", attempt.ended_as()))?;
        attempts.push(attempt);
    }
    Ok(Repair::Found(attempts))
}

/// One Account: the login, and then what Perch says about it afterwards.
///
/// The listing decides, not the exit code. An abandoned login and a repair that
/// worked have to be told apart by what the machine holds now — and a `relogin`
/// that exited nought over an Account Perch still calls Quarantined is a
/// disagreement worth catching rather than one to take the command's word on.
fn repair_one(
    perch: &Perch<'_>,
    client: &Client,
    account: String,
    was_quarantined_because: String,
    out: &mut dyn Write,
) -> Result<Attempt> {
    let attempted = |exited, ended| Attempt {
        account: account.clone(),
        was_quarantined_because: was_quarantined_because.clone(),
        exited,
        ended,
    };

    say(
        out,
        &format!(
            "  {}",
            Attempt::what_was_wrong(&account, &was_quarantined_because)
        ),
    )?;

    // `relogin` probes for a Claude Code before it starts, so a machine without
    // one records that as why this Account is Quarantined still rather than
    // being walked through a login that was never going to happen.
    if let Client::Absent(why) = client {
        return Ok(attempted(
            None,
            Ending::StillQuarantined {
                fault: Fault::Upstream,
                why: format!("there is no Claude Code to log in with: {why}"),
            },
        ));
    }

    say(
        out,
        &format!("  Logging it in again — the browser will ask you for {account}."),
    )?;

    let exited = match perch.interactive(&["relogin", &account]) {
        Ok(status) => status,
        // A binary that could not be executed at all: nothing about this
        // Account or about Anthropic is involved in that.
        Err(setback) => {
            return Ok(attempted(
                None,
                Ending::StillQuarantined {
                    fault: setback.fault,
                    why: setback.because,
                },
            ));
        }
    };

    // Three answers, not two: Perch may list it as healthy, list it as
    // Quarantined still, or not list it at all — `relogin` refuses when the
    // Account was removed while the login was happening, and that is not a
    // repair however cleanly it reads off "is it Quarantined?".
    let ending = match listed_now(perch) {
        Ok(listed) => match listed.into_iter().find(|(email, _)| email == &account) {
            None => Ending::NoLongerListed,
            Some((_, None)) => Ending::Cleared,
            Some((_, Some(why))) => still_quarantined(exited, why),
        },
        Err(setback) => still_quarantined(
            exited,
            format!(
                "the listing could not be read afterwards: {}",
                setback.because
            ),
        ),
    };
    Ok(attempted(Some(exited), ending))
}

/// An Account the login did not get back, said with whose fault that was.
///
/// A `relogin` that exited nought over an Account Perch still calls Quarantined
/// is the one reading of this that is nobody's news but Perch's: the command
/// said it had done the thing, and the listing says it had not.
fn still_quarantined(exited: i32, why: String) -> Ending {
    if exited == crate::error::EXIT_OK {
        return Ending::StillQuarantined {
            fault: Fault::Perch,
            why: format!("{why} — and `perch relogin` exited 0 over it"),
        };
    }
    let (fault, refusal) = refused_with(exited);
    Ending::StillQuarantined {
        fault,
        why: format!("{refusal} ({why})"),
    }
}

/// Every Account Perch lists, with why it is Quarantined where it is, in the
/// order the listing put them.
///
/// The whole listing rather than the Quarantined part of it, because absent and
/// healthy are different answers and only one of them is a repair.
fn listed_now(perch: &Perch<'_>) -> std::result::Result<Vec<(String, Option<String>)>, Setback> {
    let listing = perch.json(&["list", "--json"])?;
    let accounts = listing["accounts"].as_array().ok_or_else(|| {
        Setback::perch("`perch list --json` printed no `accounts` array".to_string())
    })?;

    accounts
        .iter()
        .map(|account| {
            let email = account["email"].as_str().ok_or_else(|| {
                Setback::perch(format!(
                    "`perch list --json` listed a nameless Account: {account}"
                ))
            })?;
            // Null where an Account is healthy, which is the whole of the
            // question being asked here.
            let quarantine = &account["quarantined"];
            if quarantine.is_null() {
                return Ok((email.to_string(), None));
            }
            // `detail` and not a fallback to `reason`: one binary writes this
            // document and it always writes both, so a `detail` that is not
            // there is Perch disagreeing with itself rather than an older shape
            // to tolerate.
            let why = quarantine["detail"].as_str().ok_or_else(|| {
                Setback::perch(format!(
                    "`perch list --json` said {email} is Quarantined without saying \
                     why: {quarantine}"
                ))
            })?;
            Ok((email.to_string(), Some(why.to_string())))
        })
        .collect()
}

/// What a phase proved, or what stopped it.
pub type Proof = std::result::Result<Vec<String>, Setback>;

/// One phase: a name, what it needs of the machine, and the proving.
pub struct Phase {
    pub name: &'static str,
    pub needs: Needs,
    pub prove: fn(&Perch<'_>) -> Proof,
}

/// What became of one phase in one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It ran, and these are the things it established.
    Proved(Vec<String>),
    /// This machine cannot prove it, for this reason. Counted in the Preflight
    /// figure rather than left as a line somebody scrolls past.
    Skipped(String),
    /// It stopped, and the run stopped with it.
    Stopped(Setback),
    /// It never started, because an earlier phase stopped.
    NotRun,
}

impl Outcome {
    fn word(&self) -> &'static str {
        match self {
            Outcome::Proved(_) => "proved",
            Outcome::Skipped(_) => "skipped",
            Outcome::Stopped(_) => "stopped",
            Outcome::NotRun => "not run",
        }
    }
}

/// One Dogfood run, whole — and what a report is written from.
#[derive(Debug)]
pub struct Run {
    pub began: DateTime<Utc>,
    pub marker: Marker,
    pub bin: PathBuf,
    /// The machine as the run found it.
    pub preflight: Preflight,
    /// What opening the run came to. Its own field rather than another entry in
    /// [`Run::outcomes`]: the two are read differently, and a report that folded
    /// them together would invite somebody to add a second thing that "opens the
    /// run".
    pub repair: Repair,
    /// The machine as the Repair left it, and what the phases were measured
    /// against. Only the registry is asked again — the client and the network do
    /// not change inside one run, and each costs a process or a round trip.
    pub after: Preflight,
    pub outcomes: Vec<(&'static str, Outcome)>,
}

impl Run {
    /// The phase that stopped the run, if one did. What the suite fails on.
    pub fn stopped(&self) -> Option<(&str, &Setback)> {
        self.outcomes
            .iter()
            .find_map(|(name, outcome)| match outcome {
                Outcome::Stopped(setback) => Some((*name, setback)),
                _ => None,
            })
    }

    /// What the run is written down as. Names real Accounts, which is why the
    /// directory it goes in is gitignored.
    pub fn report(&self, phases: &[Phase]) -> String {
        let mut lines = vec![
            format!("# Dogfood — {}", stamp(self.began)),
            String::new(),
            format!(
                "- Marked {}, {}",
                stamp(self.marker.marked_at),
                self.marker.export.said()
            ),
            format!("- Marked by Perch {}", self.marker.perch_version),
            format!("- Under test: {}", self.bin.display()),
            String::new(),
            "## Preflight".to_string(),
            String::new(),
        ];
        lines.extend(
            self.preflight
                .said(phases)
                .iter()
                .map(|line| format!("- {line}")),
        );
        lines.push(String::new());
        lines.push("## Repair".to_string());
        lines.push(String::new());
        lines.extend(self.repair.said().iter().map(|line| format!("- {line}")));
        // The second figure, so that a run against two Accounts and a run
        // against two Accounts one of which was dead do not read alike.
        lines.push(format!("- {}", self.after.figure_after_a_repair(phases)));

        let by_hand = self.repair.finish_by_hand();
        if !by_hand.is_empty() {
            lines.push(String::new());
            lines.extend(to_type(FINISH_BY_HAND, &by_hand));
        }

        lines.push(String::new());
        lines.push("## Phases".to_string());

        for (name, outcome) in &self.outcomes {
            lines.push(String::new());
            lines.push(format!("### {name} — {}", outcome.word()));
            lines.push(String::new());
            match outcome {
                Outcome::Proved(established) => {
                    lines.extend(established.iter().map(|line| format!("- {line}")));
                }
                Outcome::Skipped(why) => lines.push(format!("- {why}")),
                Outcome::Stopped(setback) => {
                    lines.extend(setback.said().lines().map(str::to_string))
                }
                Outcome::NotRun => {
                    lines.push("- an earlier phase stopped the run".to_string());
                }
            }
        }

        lines.push(String::new());
        lines.join("\n")
    }

    /// What the report is called: dated, and named after the machine, so a
    /// directory of them reads as a matrix rather than as a pile of dates.
    pub fn file_name(&self) -> String {
        format!(
            "dogfood-{}-{}.md",
            stamp(self.began).replace(':', "-"),
            self.preflight.machine()
        )
    }
}

fn stamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// A Dogfood run, from the refusal to the report.
///
/// The marker first, before the Preflight and long before a phase touches
/// anything. Then what the machine holds, said out loud as a figure. Then the
/// Repair, which clears what yesterday's run on another machine retired and
/// changes the figure rather than the run's fate. Then the
/// phases, one at a time, stopping at the first that does — and the report,
/// written whatever happened, because a run that stopped is the one worth
/// having written down.
///
/// What one phase came to, measured against the machine as it stands rather
/// than as the Repair left it.
fn prove(perch: &Perch<'_>, standing: &Preflight, phase: &Phase) -> Outcome {
    if let Some(why) = standing.unmet(&phase.needs) {
        return Outcome::Skipped(why);
    }
    match (phase.prove)(perch) {
        Ok(established) => Outcome::Proved(established),
        Err(setback) => Outcome::Stopped(setback),
    }
}

pub fn dogfood(
    host: &dyn Host,
    built: &str,
    phases: &[Phase],
    reports: &Path,
    out: &mut dyn Write,
) -> Result<Run> {
    let marker = marker(host)?;
    // Read before the phases rather than after them, so a report is named after
    // when the run started. A full run is somebody sitting there for an hour,
    // and a stamp taken at the end names the wrong sitting.
    let began = host.now();
    let preflight = Preflight::taken(host)?;
    let perch = Perch::under_test(host, built);

    say(out, &format!("Under test: {}", perch.bin().display()))?;
    for line in preflight.said(phases) {
        say(out, &line)?;
    }

    let repaired = repair(&perch, &preflight, out)?;

    // The machine as the Repair left it, which is the figure the run reports: a
    // figure taken before it would count an Account nobody could log back in as
    // one this machine can prove things with. It is a sentence about what the
    // machine could prove when the run started, and deliberately not a running
    // total — `standing` below is what each phase is actually gated on.
    let mut after = preflight.clone();
    after.read_the_registry(host)?;
    say(out, "")?;
    say(out, &after.figure_after_a_repair(phases))?;

    // The machine as the phase before it left it. A phase that Switches changes
    // which Account is active, and one that walks a Renewal can Rotate and leave
    // a `RenewalRejected` Quarantine behind — so gating every phase on the
    // snapshot taken before the first of them ran admits phase n+1 on facts
    // phase n has already invalidated, and runs it against a Quarantined
    // Account. That is the reading `usable` was added to prevent, closed against
    // yesterday's run on another machine and left open against the phase two
    // lines above.
    let mut standing = after.clone();

    let mut outcomes: Vec<(&'static str, Outcome)> = Vec::new();
    let mut stopped = false;
    for phase in phases {
        let outcome = if stopped {
            Outcome::NotRun
        } else if let Err(why) = standing.read_the_registry(host) {
            // Not a `?`: everything before this point has already touched the
            // machine, and a registry that will not be read is the moment a
            // report is worth most rather than the moment to throw one away.
            Outcome::Stopped(Setback::perch(format!(
                "the registry could not be read between phases: {why}"
            )))
        } else {
            prove(&perch, &standing, phase)
        };
        stopped = stopped || matches!(outcome, Outcome::Stopped(_));

        say(out, "")?;
        say(out, &format!("{} — {}", phase.name, outcome.word()))?;
        match &outcome {
            Outcome::Proved(established) => {
                for line in established {
                    say(out, &format!("  {line}"))?;
                }
            }
            Outcome::Skipped(why) => say(out, &format!("  {why}"))?,
            Outcome::Stopped(setback) => say(out, &setback.said())?,
            Outcome::NotRun => {}
        }
        outcomes.push((phase.name, outcome));
    }

    let run = Run {
        began,
        marker,
        bin: perch.bin().to_path_buf(),
        preflight,
        repair: repaired,
        after,
        outcomes,
    };
    let written = write_the_report(host, &run, phases, reports)?;
    say(out, "")?;
    say(out, &format!("Report: {}", written.display()))?;

    Ok(run)
}

// ---- the wizard ------------------------------------------------------------

/// What `dogfood-setup` was asked for.
#[derive(Debug, Clone, Default)]
pub struct SetupArgs {
    /// Where the Export goes. Asked for at the terminal when it is not given.
    pub export_to: Option<PathBuf>,
    /// Ask nothing, take no Export, and walk no login.
    ///
    /// For CI, and refused anywhere it would skip an Export that had something
    /// to save: a machine holding Accounts is set up by somebody watching, or
    /// not at all.
    pub unattended: bool,
}

/// Marks this machine for Dogfood runs — and everything that has to be true
/// before it may be marked.
///
/// The Export is the whole of the guarantee, so it is taken over whatever this
/// machine holds and the marker is written after it. A machine holding nothing
/// has nothing an Export could save; there, a login is offered first, and the
/// Export follows if one arrives. Either way the marker means the same thing:
/// every Account on this machine at the moment it was written is in a file.
pub fn set_up(
    host: &dyn Host,
    built: &str,
    args: &SetupArgs,
    phases: &[Phase],
    out: &mut dyn Write,
) -> Result<Marker> {
    let perch = Perch::under_test(host, built);
    refuse_a_binary_that_is_not_there(host, &perch)?;
    // Taken once and carried through. The client and the network are a process
    // and a round trip apiece, and neither answers differently for the length of
    // a setup; what a login changes is the registry, which is re-read where it
    // changes.
    let mut preflight = Preflight::taken(host)?;

    say(
        out,
        &format!("Setting up {} for Dogfood runs.", perch.bin().display()),
    )?;
    say(out, "")?;

    let export = if args.unattended {
        refuse_an_unattended_setup_that_would_skip_an_export(&preflight.accounts)?;
        say(
            out,
            "Unattended: Perch holds no Accounts here, so there is nothing an \
             Export could save and no login to walk.",
        )?;
        Held::NothingHeld
    } else if preflight.accounts.is_empty() {
        say(
            out,
            &format!(
                "Perch holds no Accounts here, so there is nothing an Export \
                 could save yet.\nA Dogfood run on this machine can prove {} of \
                 {} as it stands.",
                preflight.provable(phases),
                counted(phases.len())
            ),
        )?;
        walk_a_login(host, &perch, args, &mut preflight, out)?
    } else {
        say(
            out,
            &format!(
                "Perch holds {} here. An Export comes before anything else, \
                 because it is the only thing that makes the rest reversible.",
                crate::commands::accounts(preflight.accounts.len())
            ),
        )?;
        Held::Exported {
            path: take_an_export(host, &perch, args, out)?,
        }
    };

    let marker = Marker {
        version: MARKER_VERSION,
        marked_at: host.now(),
        perch_version: env!("CARGO_PKG_VERSION").to_string(),
        export,
    };
    mark(host, &marker)?;

    say(out, "")?;
    say(out, &format!("Marked: {}", marker_path(host)?.display()))?;
    say(out, &marker.export.said())?;
    say(out, "")?;
    // The figure, said at the end of setup as well as at the start of a run:
    // whoever just walked this is the person who can do something about it.
    for line in preflight.said(phases) {
        say(out, &line)?;
    }

    Ok(marker)
}

/// The footgun this closes: `cargo run --bin dogfood-setup` builds that binary
/// and not `perch` beside it, so the wizard would find nothing to drive and say
/// so first at the Export — after somebody had typed a passphrase twice.
///
/// Only for a path. A bare name is `PATH`'s to resolve, and asking here whether
/// something is on `PATH` is a second search that can disagree with the one the
/// operating system does.
fn refuse_a_binary_that_is_not_there(host: &dyn Host, perch: &Perch<'_>) -> Result<()> {
    let bin = perch.bin();
    if bin
        .parent()
        .is_none_or(|parent| parent.as_os_str().is_empty())
        || host.path_exists(bin)
    {
        return Ok(());
    }
    Err(PerchError::NotFound(format!(
        "There is no Perch at {} to set this machine up against.\n\
         `cargo run --bin dogfood-setup` builds the wizard and not the binary it \
         drives. Run `cargo build --features dogfood --bins` first, or point \
         {BIN_VARIABLE} at an installed Perch.",
        bin.display()
    )))
}

/// An unattended setup may skip the Export only where there was nothing for it
/// to save. Anywhere else it is refused, because a marker written without one is
/// the marker being a flag anybody could set rather than the wizard's receipt.
fn refuse_an_unattended_setup_that_would_skip_an_export(held: &[String]) -> Result<()> {
    if held.is_empty() {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "Perch holds {} on this machine, and an unattended setup takes no \
         Export.\nA Dogfood run moves real Credentials around, so the machine \
         it runs on is one somebody set up while watching. Run \
         `dogfood-setup` without `--unattended`, where you can type.",
        crate::commands::accounts(held.len())
    )))
}

/// A machine with nothing on it: offer a login, and Export if one arrives.
fn walk_a_login(
    host: &dyn Host,
    perch: &Perch<'_>,
    args: &SetupArgs,
    preflight: &mut Preflight,
    out: &mut dyn Write,
) -> Result<Held> {
    let answer = crate::commands::ask_a_word(host, out, "Log an Account in now? [y/N] ")?;
    if !matches!(answer.as_deref(), Some("y" | "yes")) {
        say(out, "No login walked.")?;
        return Ok(Held::NothingHeld);
    }

    let status = perch
        .interactive(&["add"])
        .map_err(|setback| PerchError::Other(setback.because))?;
    if status != crate::error::EXIT_OK {
        return Err(PerchError::Other(format!(
            "`perch add` exited {status}, so this machine holds no more than it \
             did and has not been marked."
        )));
    }

    // Asked again rather than assumed: an abandoned login exits cleanly and adds
    // nothing, and a marker claiming an Export of an Account that is not there
    // is worse than one that says nothing was held.
    preflight.read_the_registry(host)?;
    if preflight.accounts.is_empty() {
        say(out, "The login added no Account.")?;
        return Ok(Held::NothingHeld);
    }

    say(out, "")?;
    say(
        out,
        "That Account is now the only copy of a login on this machine. An \
         Export is what makes the rest of a Dogfood run reversible.",
    )?;
    Ok(Held::Exported {
        path: take_an_export(host, perch, args, out)?,
    })
}

/// Hands the terminal to `perch export`, which is where the passphrase is
/// typed. Perch never sees it and neither does the wizard.
fn take_an_export(
    host: &dyn Host,
    perch: &Perch<'_>,
    args: &SetupArgs,
    out: &mut dyn Write,
) -> Result<PathBuf> {
    let path = match &args.export_to {
        Some(path) => path.clone(),
        None => {
            let default = default_export_path(host);
            let answered = crate::commands::ask(
                host,
                out,
                &format!("Where should the Export go? [{}] ", default.display()),
            )?;
            match answered.as_deref().map(str::trim) {
                Some("") | None => default,
                Some(typed) => PathBuf::from(typed),
            }
        }
    };

    let named = path.display().to_string();
    let status = perch
        .interactive(&["export", &named])
        .map_err(|setback| PerchError::Other(setback.because))?;
    if status != crate::error::EXIT_OK {
        return Err(PerchError::Other(format!(
            "`perch export {named}` exited {status}. Nothing has been marked: \
             the Export is the whole of what a marker stands for."
        )));
    }

    Ok(path)
}

/// Where an Export goes when nobody says. In the home directory rather than
/// under `~/.config/perch`, which is exactly what `perch purge` deletes.
fn default_export_path(host: &dyn Host) -> PathBuf {
    let named = format!(
        "perch-dogfood-{}.age",
        host.now().format("%Y-%m-%dT%H-%M-%SZ")
    );
    match host.home_dir() {
        Ok(home) => home.join(named),
        Err(_) => PathBuf::from(named),
    }
}

fn say(out: &mut dyn Write, line: &str) -> Result<()> {
    writeln!(out, "{line}").map_err(|err| PerchError::Other(err.to_string()))
}

fn write_the_report(
    host: &dyn Host,
    run: &Run,
    phases: &[Phase],
    reports: &Path,
) -> Result<PathBuf> {
    host.create_dir_all(reports)
        .map_err(|err| PerchError::file_write(reports, err))?;
    let path = reports.join(run.file_name());
    crate::host::write_atomically(host, &path, &run.report(phases))
        .map_err(|err| PerchError::file_write(&path, err))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Execution, FakeHost};
    use crate::registry::Quarantine;

    fn a_marker() -> Marker {
        Marker {
            version: MARKER_VERSION,
            marked_at: "2026-08-12T09:00:00Z".parse().unwrap(),
            perch_version: "0.1.1".to_string(),
            export: Held::Exported {
                path: PathBuf::from("/Users/someone/perch-2026-08-12.age"),
            },
        }
    }

    #[test]
    fn an_unmarked_machine_is_refused_and_told_how_to_be_marked() {
        let host = FakeHost::new().with_env("PERCH_HOME", "/tmp/perch");

        let refused = marker(&host).expect_err("nothing has marked this machine");

        assert!(matches!(refused, PerchError::NotFound(_)));
        assert!(
            refused.to_string().contains("dogfood-setup"),
            "a refusal nobody can act on: {refused}"
        );
    }

    #[test]
    fn what_the_wizard_wrote_is_what_the_suite_reads() {
        let host = FakeHost::new().with_env("PERCH_HOME", "/tmp/perch");

        mark(&host, &a_marker()).expect("the marker is written");

        assert_eq!(marker(&host).expect("and read back"), a_marker());
    }

    #[test]
    fn a_marker_from_a_newer_perch_is_refused_rather_than_misread() {
        let host = FakeHost::new()
            .with_env("PERCH_HOME", "/tmp/perch")
            .with_file(
                "/tmp/perch/dogfood.json",
                r#"{"version": 2, "marked_at": "2026-08-12T09:00:00Z",
                    "perch_version": "9.9.9", "export": {"kind": "whatever-comes-next"}}"#,
            );

        let refused = marker(&host).expect_err("a marker from the future");

        assert!(
            refused.to_string().contains("newer Perch"),
            "the version guard has to fire before serde does: {refused}"
        );
    }

    #[test]
    fn a_marker_that_will_not_parse_names_the_file() {
        let host = FakeHost::new()
            .with_env("PERCH_HOME", "/tmp/perch")
            .with_file("/tmp/perch/dogfood.json", "{not json");

        let refused = marker(&host).expect_err("nothing can be read out of that");

        assert!(matches!(refused, PerchError::Malformed { .. }));
        assert!(refused.to_string().contains("dogfood.json"), "{refused}");
    }

    // ---- what the machine holds -------------------------------------------

    /// A machine with nothing on it: no Claude Code, no network, no registry.
    /// The CI runner, in other words, and the one every degradation is measured
    /// against.
    fn a_bare_machine() -> FakeHost {
        FakeHost::new()
            .with_env("PERCH_HOME", "/tmp/perch")
            .with_env("PATH", "/nowhere")
            .with_env("HOME", "/Users/someone")
    }

    fn an_account(email: &str, quarantine: Option<Quarantine>) -> crate::registry::Account {
        crate::registry::Account {
            identity: crate::probe::Identity {
                email: email.to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            enabled: true,
            quarantine,
            group: None,
            utilization: None,
        }
    }

    fn holding(host: FakeHost, accounts: &[crate::registry::Account]) -> FakeHost {
        let mut registry = crate::registry::Registry::default();
        for account in accounts {
            registry.upsert(account.clone());
        }
        // The ordinary machine: whichever Account was added first is the one
        // somebody is on. `holding_none_active` is the other case.
        registry.active = accounts.first().map(|account| account.email().to_string());
        host.with_file(
            "/tmp/perch/registry.json",
            &serde_json::to_string(&registry).unwrap(),
        )
    }

    /// A Claude Code that answers, which `perch relogin` probes for before it
    /// will start.
    fn with_a_claude_code(host: FakeHost) -> FakeHost {
        host.with_env("PERCH_CLAUDE_BIN", "/bin/claude").with_exec(
            "/bin/claude",
            &["--version"],
            Execution {
                status: 0,
                stdout: "2.1.221 (Claude Code)".to_string(),
                stderr: String::new(),
            },
        )
    }

    #[test]
    fn a_machine_with_nothing_on_it_says_so_rather_than_failing_to_be_read() {
        let preflight = Preflight::taken(&a_bare_machine()).expect("a bare machine is describable");

        assert!(matches!(preflight.client, Client::Absent(_)));
        assert!(matches!(preflight.network, Answering::No(_)));
        assert!(preflight.accounts.is_empty());
        assert!(preflight.quarantined.is_empty());
    }

    #[test]
    fn every_account_is_counted_and_the_quarantined_ones_are_named() {
        let host = holding(
            a_bare_machine(),
            &[
                an_account("one@example.com", None),
                an_account("two@example.com", Some(Quarantine::RenewalRejected)),
            ],
        );

        let preflight = Preflight::taken(&host).expect("the registry reads");

        assert_eq!(
            preflight.accounts.len(),
            2,
            "a Quarantine is still an Account"
        );
        assert_eq!(preflight.quarantined, vec!["two@example.com".to_string()]);
    }

    #[test]
    fn wsl_is_read_from_the_variables_it_sets_and_from_proc_version() {
        assert!(!is_wsl(&a_bare_machine()));
        assert!(is_wsl(
            &a_bare_machine().with_env("WSL_DISTRO_NAME", "Ubuntu")
        ));
        assert!(is_wsl(&a_bare_machine().with_file(
            "/proc/version",
            "Linux version 5.15.0-microsoft-standard"
        )));
    }

    #[test]
    fn a_machine_is_named_after_what_it_is_rather_than_after_its_platform_alone() {
        let mut preflight = Preflight::taken(&a_bare_machine()).unwrap();
        preflight.platform = Platform::MacOs;
        assert_eq!(preflight.machine(), "macos");

        preflight.platform = Platform::Other;
        assert_eq!(preflight.machine(), "linux");
        preflight.wsl = true;
        assert_eq!(
            preflight.machine(),
            "wsl",
            "WSL is its own machine, not a Linux one: it has a phase Linux has not"
        );
    }

    // ---- what a machine can prove -----------------------------------------

    fn proves_nothing(_: &Perch<'_>) -> Proof {
        Ok(vec!["nothing worth saying".to_string()])
    }

    fn a_phase(name: &'static str, needs: Needs) -> Phase {
        Phase {
            name,
            needs,
            prove: proves_nothing,
        }
    }

    #[test]
    fn a_phase_needing_more_accounts_than_are_held_says_which_of_the_two_numbers_is_short() {
        let preflight = Preflight::taken(&a_bare_machine()).unwrap();

        assert_eq!(preflight.unmet(&Needs::NOTHING), None);
        let unmet = preflight
            .unmet(&Needs::AN_ACCOUNT)
            .expect("nothing is held here");
        assert_eq!(
            unmet,
            "Perch holds 0 Accounts here and this phase needs 1 Account"
        );
    }

    /// Holding Accounts and being on one are two different things, and `perch
    /// status` is a refusal rather than a document when the second is not true.
    /// A phase that reads one is skipped here rather than stopped halfway.
    #[test]
    fn a_machine_holding_accounts_with_none_active_still_cannot_prove_a_status_phase() {
        let mut registry = crate::registry::Registry::default();
        registry.upsert(an_account("one@example.com", None));
        let host = a_bare_machine().with_file(
            "/tmp/perch/registry.json",
            &serde_json::to_string(&registry).unwrap(),
        );

        let preflight = Preflight::taken(&host).unwrap();

        assert_eq!(preflight.active, None);
        assert_eq!(preflight.unmet(&Needs::AN_ACCOUNT), None);
        assert!(
            preflight
                .unmet(&Needs::THE_ACTIVE_ACCOUNT)
                .is_some_and(|why| why.contains("no Account is active"))
        );
    }

    #[test]
    fn a_client_and_a_network_are_each_their_own_reason_to_skip() {
        let preflight = Preflight::taken(&a_bare_machine()).unwrap();

        let needs_a_client = Needs {
            client: true,
            ..Needs::NOTHING
        };
        assert!(
            preflight
                .unmet(&needs_a_client)
                .is_some_and(|why| why.contains("Claude Code"))
        );

        let needs_the_network = Needs {
            network: true,
            ..Needs::NOTHING
        };
        assert!(
            preflight
                .unmet(&needs_the_network)
                .is_some_and(|why| why.contains("Anthropic"))
        );
    }

    /// The failure the Preflight exists to prevent: a green run that quietly
    /// proved a third of what it was asked to. So the figure is said, and it is
    /// said as a figure.
    #[test]
    fn how_much_this_machine_can_prove_is_said_as_a_number_of_phases() {
        let phases = [
            a_phase("runs anywhere", Needs::NOTHING),
            a_phase("needs an Account", Needs::AN_ACCOUNT),
        ];

        let bare = Preflight::taken(&a_bare_machine()).unwrap();
        assert!(
            bare.said(&phases)
                .iter()
                .any(|line| line == "This machine can prove 1 of 2 phases"),
            "{:?}",
            bare.said(&phases)
        );

        let logged_in = Preflight::taken(&holding(
            a_bare_machine(),
            &[an_account("one@example.com", None)],
        ))
        .unwrap();
        assert!(
            logged_in
                .said(&phases)
                .iter()
                .any(|line| line == "This machine can prove 2 of 2 phases")
        );
    }

    // ---- the run ----------------------------------------------------------

    fn marked(host: FakeHost) -> FakeHost {
        mark(&host, &a_marker()).expect("the wizard marked it");
        host
    }

    fn a_run(host: &dyn Host, phases: &[Phase]) -> Result<Run> {
        dogfood(
            host,
            "/build/perch",
            phases,
            Path::new("/tmp/reports"),
            &mut Vec::new(),
        )
    }

    #[test]
    fn an_unmarked_machine_is_refused_before_a_phase_can_touch_it() {
        fn touches_it(_: &Perch<'_>) -> Proof {
            panic!("an unmarked machine must not reach a phase");
        }

        let refused = a_run(
            &a_bare_machine(),
            &[Phase {
                name: "would act",
                needs: Needs::NOTHING,
                prove: touches_it,
            }],
        )
        .expect_err("this machine is unmarked");

        assert!(refused.to_string().contains("dogfood-setup"), "{refused}");
    }

    #[test]
    fn the_binary_under_test_is_the_one_that_was_built_unless_it_is_pointed_elsewhere() {
        let host = a_bare_machine();
        assert_eq!(
            Perch::under_test(&host, "/build/perch").bin(),
            Path::new("/build/perch")
        );

        let installed = a_bare_machine().with_env(BIN_VARIABLE, "/usr/local/bin/perch");
        assert_eq!(
            Perch::under_test(&installed, "/build/perch").bin(),
            Path::new("/usr/local/bin/perch"),
            "the only way a bug in the release archive itself is ever caught"
        );
    }

    /// A `perch` under test that exits with `status` and says `said` when a
    /// phase reads a listing through it.
    fn exiting(status: i32, said: &str) -> FakeHost {
        a_bare_machine().with_exec(
            "/build/perch",
            &["list", "--json"],
            Execution {
                status,
                stdout: String::new(),
                stderr: said.to_string(),
            },
        )
    }

    #[test]
    fn a_listing_another_perch_was_holding_is_news_rather_than_a_defect() {
        let host = exiting(
            crate::error::EXIT_HELD,
            "Another perch is holding the registry.",
        );

        let setback = Perch::under_test(&host, "/build/perch")
            .json(&["list", "--json"])
            .expect_err("it exited non-zero");

        assert_eq!(
            setback.fault,
            Fault::Upstream,
            "a machine somebody works on runs other Perches: {}",
            setback.because
        );
        assert!(setback.said().contains("not a fault in Perch"));
    }

    #[test]
    fn a_listing_that_refused_for_a_reason_the_preflight_ruled_out_is_a_defect() {
        let host = exiting(crate::error::EXIT_NOT_FOUND, "No such Account.");

        let setback = Perch::under_test(&host, "/build/perch")
            .json(&["list", "--json"])
            .expect_err("it exited non-zero");

        assert_eq!(
            setback.fault,
            Fault::Perch,
            "the Preflight read these Accounts through the same binary: {}",
            setback.because
        );
    }

    /// The reading `usable` was added for, closed against the phase two lines
    /// above rather than only against yesterday's run on another machine.
    #[test]
    fn a_phase_is_gated_on_the_machine_the_phase_before_it_left() {
        fn breaks_one(perch: &Perch<'_>) -> Proof {
            perch.interactive(&["relogin", "two@example.com"])?;
            Ok(vec!["it left an Account Quarantined".to_string()])
        }

        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine()))).with_login(
            |host, _| {
                now_holding(
                    host,
                    &[
                        ("one@example.com".to_string(), None),
                        (
                            "two@example.com".to_string(),
                            Some(Quarantine::RenewalRejected),
                        ),
                    ],
                );
                crate::error::EXIT_OK
            },
        );
        now_holding(
            &host,
            &[
                ("one@example.com".to_string(), None),
                ("two@example.com".to_string(), None),
            ],
        );

        let both = Needs {
            accounts: 2,
            ..Needs::NOTHING
        };
        let run = a_run(
            &host,
            &[
                Phase {
                    name: "leaves one broken",
                    needs: both,
                    prove: breaks_one,
                },
                a_phase("needs both", both),
            ],
        )
        .expect("the run finished");

        assert!(
            matches!(run.outcomes[0].1, Outcome::Proved(_)),
            "the first phase had both Accounts: {:?}",
            run.outcomes[0].1
        );
        let Outcome::Skipped(why) = &run.outcomes[1].1 else {
            panic!(
                "the second phase must be measured against what the first left: {:?}",
                run.outcomes[1].1
            );
        };
        assert!(why.contains('2') && why.contains('1'), "{why}");
    }

    #[test]
    fn a_phase_this_machine_cannot_prove_is_skipped_and_the_rest_still_run() {
        let host = marked(a_bare_machine());

        let run = a_run(
            &host,
            &[
                a_phase("needs an Account", Needs::AN_ACCOUNT),
                a_phase("runs anywhere", Needs::NOTHING),
            ],
        )
        .expect("a marked machine runs");

        assert!(matches!(run.outcomes[0].1, Outcome::Skipped(_)));
        assert!(matches!(run.outcomes[1].1, Outcome::Proved(_)));
        assert_eq!(run.stopped(), None, "a skip is not a failure");
    }

    /// It never unwinds itself: an unwind can fail too, and a failed unwind
    /// after a failed assertion leaves a state nobody can read (ADR 0037).
    #[test]
    fn a_phase_that_stops_stops_the_run_and_says_what_puts_it_back() {
        fn stops(_: &Perch<'_>) -> Proof {
            Err(Setback::perch("`perch switch` landed on the wrong Account")
                .leaving(&["two@example.com is active".to_string()])
                .put_back_with(&["perch switch one@example.com".to_string()]))
        }

        let host = marked(a_bare_machine());
        let run = a_run(
            &host,
            &[
                Phase {
                    name: "stops",
                    needs: Needs::NOTHING,
                    prove: stops,
                },
                a_phase("after it", Needs::NOTHING),
            ],
        )
        .expect("a marked machine runs");

        let (name, setback) = run.stopped().expect("the first phase stopped");
        assert_eq!(name, "stops");
        assert_eq!(setback.fault, Fault::Perch);
        assert!(setback.said().contains("$ perch switch one@example.com"));
        assert_eq!(
            run.outcomes[1].1,
            Outcome::NotRun,
            "the run stops rather than carrying on over a machine nobody has read"
        );
    }

    #[test]
    fn a_setback_says_which_of_the_two_kinds_of_failure_it_is() {
        assert!(
            Setback::upstream("Anthropic timed out")
                .said()
                .contains("not a fault in Perch")
        );
        assert!(
            Setback::perch("the exit code was 1")
                .said()
                .contains("This is a fault in Perch.")
        );
        assert!(
            Setback::perch("the exit code was 1")
                .said()
                .contains("Nothing on this machine was changed."),
            "a phase that changed nothing has to say so, or somebody goes looking"
        );
    }

    /// It names real Accounts, which is the whole reason the directory it goes
    /// in is gitignored.
    #[test]
    fn the_report_is_dated_named_after_the_machine_and_written_whatever_happened() {
        let host = marked(holding(
            a_bare_machine(),
            &[an_account("someone@example.com", None)],
        ));
        let phases = [a_phase("runs anywhere", Needs::NOTHING)];

        let run = a_run(&host, &phases).expect("a marked machine runs");
        let at = Path::new("/tmp/reports").join(run.file_name());

        assert!(
            run.file_name().starts_with("dogfood-"),
            "{}",
            run.file_name()
        );
        assert!(!run.file_name().contains(':'), "Windows will not have it");
        let written = host.read_file(&at).expect("the report was written");
        assert_eq!(written, run.report(&phases));
        assert!(written.contains("someone@example.com"));
        assert!(
            written.contains("This machine can prove 1 of 1 phase"),
            "one phase is a phase, not a phase(s): {written}"
        );
        assert!(written.contains("### runs anywhere — proved"));
    }

    // ---- phase zero: the Repair -------------------------------------------
    //
    // Everything here is driven through the run loop against a fake machine, and
    // asserts what somebody watching would see: which Accounts the Repair
    // attempted, what it reported for each, what the figure said before and
    // after, and which phases then ran or skipped and why. Nothing names a
    // function, so none of it breaks because the Repair was reorganised.

    /// What a `perch relogin` the Repair walks comes to.
    #[derive(Debug, Clone, Copy)]
    enum Login {
        /// It works, and the Account is no longer Quarantined afterwards.
        Works,
        /// It does not, and the Account is exactly as broken as it was — a login
        /// somebody abandoned, a client still holding the Profile, a browser
        /// signed in as somebody else.
        Leaves(i32),
        /// The Account was removed while the login was happening, which is a
        /// refusal `perch relogin` already makes: the login worked and there is
        /// nothing left to repair in place.
        Removes,
    }

    /// A machine holding these Accounts, where `perch list --json` says what the
    /// registry says and `perch relogin` repairs whichever Account it was
    /// pointed at unless `logins` scripts it otherwise.
    ///
    /// The two moving together is the whole of what makes this a machine rather
    /// than two canned documents: the Repair reads the listing through the
    /// binary under test, and the figure it changes is read off the registry.
    fn a_machine(held: &[(&str, Option<Quarantine>)], logins: &[(&str, Login)]) -> FakeHost {
        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())));

        let holds: Vec<(String, Option<Quarantine>)> = held
            .iter()
            .map(|(email, quarantine)| ((*email).to_string(), *quarantine))
            .collect();
        now_holding(&host, &holds);

        let scripted: Vec<(String, Login)> = logins
            .iter()
            .map(|(email, login)| ((*email).to_string(), *login))
            .collect();
        let holds = std::cell::RefCell::new(holds);
        host.with_login(move |host, _| {
            let repairing = being_repaired(host);
            let login = scripted
                .iter()
                .find(|(email, _)| *email == repairing)
                .map_or(Login::Works, |(_, login)| *login);
            match login {
                Login::Leaves(status) => status,
                Login::Works => {
                    let mut holds = holds.borrow_mut();
                    for held in holds.iter_mut().filter(|(email, _)| *email == repairing) {
                        held.1 = None;
                    }
                    now_holding(host, &holds);
                    crate::error::EXIT_OK
                }
                Login::Removes => {
                    let mut holds = holds.borrow_mut();
                    holds.retain(|(email, _)| *email != repairing);
                    now_holding(host, &holds);
                    crate::error::EXIT_NOT_FOUND
                }
            }
        })
    }

    /// What this machine holds, written where both things that read it look: the
    /// registry the Preflight reads directly, and the `perch list --json` the
    /// Repair reads through the binary under test.
    fn now_holding(host: &FakeHost, held: &[(String, Option<Quarantine>)]) {
        let mut registry = crate::registry::Registry::default();
        for (email, quarantine) in held {
            registry.upsert(an_account(email, *quarantine));
        }
        registry.active = held.first().map(|(email, _)| email.clone());
        host.set_file(
            "/tmp/perch/registry.json",
            &serde_json::to_string(&registry).unwrap(),
        );

        let accounts: Vec<serde_json::Value> = held
            .iter()
            .map(|(email, quarantine)| {
                serde_json::json!({
                    "email": email,
                    "active": registry.active.as_deref() == Some(email.as_str()),
                    "quarantined": Quarantine::document(*quarantine),
                })
            })
            .collect();
        host.set_exec(
            "/build/perch",
            &["list", "--json"],
            Execution {
                status: crate::error::EXIT_OK,
                stdout: serde_json::json!({
                    "active_account": registry.active,
                    "accounts": accounts,
                })
                .to_string(),
                stderr: String::new(),
            },
        );
    }

    /// Which Account the `perch relogin` now running was pointed at, read off
    /// the launch the fake just recorded. A login hook stands in for a process,
    /// and a process knows its own argv.
    fn being_repaired(host: &FakeHost) -> String {
        relogins(host)
            .pop()
            .expect("the only thing this machine's login stands in for is a relogin")
    }

    /// Every `perch relogin` the run handed the terminal to, in order.
    fn relogins(host: &FakeHost) -> Vec<String> {
        host.effects()
            .iter()
            .filter_map(|effect| match effect {
                crate::host::fake::Effect::ExecInteractive { args, .. }
                    if args.first().is_some_and(|arg| arg == "relogin") =>
                {
                    args.get(1).cloned()
                }
                _ => None,
            })
            .collect()
    }

    /// A run, and everything it printed, so a test can assert on what somebody
    /// watching would have seen as well as on what came back.
    fn a_watched_run(host: &dyn Host, phases: &[Phase]) -> Result<(Run, String)> {
        let mut said = Vec::new();
        let run = dogfood(
            host,
            "/build/perch",
            phases,
            Path::new("/tmp/reports"),
            &mut said,
        )?;
        Ok((run, String::from_utf8(said).unwrap()))
    }

    #[test]
    fn a_machine_with_nothing_quarantined_says_so_in_a_line_and_walks_no_login() {
        let host = a_machine(&[("one@example.com", None)], &[]);

        let (run, said) = a_watched_run(&host, &[a_phase("needs an Account", Needs::AN_ACCOUNT)])
            .expect("a marked machine runs");

        assert_eq!(run.repair, Repair::NothingQuarantined);
        assert!(said.contains("Nothing is Quarantined here"), "{said}");
        assert!(relogins(&host).is_empty(), "nothing was there to repair");
        assert!(matches!(run.outcomes[0].1, Outcome::Proved(_)));
    }

    /// The one the CI runner takes, on every platform, on every run: nothing is
    /// held, so nothing needs a terminal and the degraded run stays green.
    #[test]
    fn a_machine_holding_no_accounts_says_there_was_nothing_to_repair() {
        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())));

        let (run, said) =
            a_watched_run(&host, &[a_phase("runs anywhere", Needs::NOTHING)]).expect("it runs");

        assert_eq!(run.repair, Repair::NothingHeld);
        assert!(said.contains("Perch holds no Accounts here"), "{said}");
        assert!(
            relogins(&host).is_empty(),
            "a skip and a success must not look alike, and neither needs a terminal"
        );
    }

    #[test]
    fn a_quarantine_that_clears_is_reported_cleared_and_the_phase_then_runs() {
        let host = a_machine(
            &[("one@example.com", Some(Quarantine::RenewalRejected))],
            &[],
        );
        let phases = [a_phase("needs an Account", Needs::AN_ACCOUNT)];

        let (run, said) = a_watched_run(&host, &phases).expect("it runs");

        assert_eq!(relogins(&host), vec!["one@example.com".to_string()]);
        assert_eq!(
            run.repair.left_quarantined(),
            Vec::<&str>::new(),
            "the login worked: {said}"
        );
        assert!(
            said.contains("one@example.com was Quarantined: Anthropic would not renew"),
            "somebody about to walk a login is owed what stopped working: {said}"
        );
        assert!(
            said.contains("the browser will ask you for one@example.com"),
            "so that they log in as the right Account: {said}"
        );
        assert!(said.contains("it is no longer Quarantined"), "{said}");
        assert!(
            said.contains("This machine can prove 0 of 1 phase"),
            "what it could prove when it was found: {said}"
        );
        assert!(
            said.contains("This machine can now prove 1 of 1 phase"),
            "and what it can prove now: {said}"
        );
        assert!(matches!(run.outcomes[0].1, Outcome::Proved(_)));
    }

    /// A Quarantine another machine's run caused is the ordinary starting state
    /// rather than a defect, so a login somebody abandoned costs the phase and
    /// nothing else (ADR 0037).
    #[test]
    fn an_abandoned_login_leaves_the_account_quarantined_and_does_not_stop_the_run() {
        let host = a_machine(
            &[("one@example.com", Some(Quarantine::RenewalRejected))],
            &[(
                "one@example.com",
                Login::Leaves(crate::error::EXIT_NOT_FOUND),
            )],
        );
        let phases = [a_phase("needs an Account", Needs::AN_ACCOUNT)];

        let (run, said) = a_watched_run(&host, &phases).expect("a Repair cannot stop a run");

        assert_eq!(run.stopped(), None, "the Repair never produces a Setback");
        assert_eq!(run.repair.left_quarantined(), vec!["one@example.com"]);
        assert!(said.contains("it is Quarantined still"), "{said}");
        assert!(
            said.contains("not a fault in Perch"),
            "a red run still has to say who has work to do: {said}"
        );
        assert!(
            said.contains("$ perch relogin one@example.com"),
            "the commands that would finish the job by hand: {said}"
        );

        let Outcome::Skipped(why) = &run.outcomes[0].1 else {
            panic!("a phase needing a usable Account cannot run here: {run:?}");
        };
        assert!(
            why.contains("one@example.com still Quarantined"),
            "the skip line names the Quarantine as the reason: {why}"
        );
        assert!(
            said.contains("This machine can now prove 0 of 1 phase"),
            "{said}"
        );
    }

    /// The property that makes a Repair worth having at all.
    #[test]
    fn a_login_that_failed_does_not_cost_the_next_account_its_repair() {
        let host = a_machine(
            &[
                ("one@example.com", Some(Quarantine::RenewalRejected)),
                ("two@example.com", Some(Quarantine::NoCredential)),
            ],
            &[(
                "one@example.com",
                Login::Leaves(crate::error::EXIT_PROFILE_LIVE),
            )],
        );

        let (run, said) = a_watched_run(&host, &[]).expect("it runs");

        assert_eq!(
            relogins(&host),
            vec!["one@example.com".to_string(), "two@example.com".to_string()],
            "one Account somebody cannot get into must not cost them the other"
        );
        assert_eq!(run.repair.left_quarantined(), vec!["one@example.com"]);
        assert!(
            said.contains("two@example.com was Quarantined: Perch holds no Credential for it"),
            "each Account is named with why it was Quarantined: {said}"
        );
        assert_eq!(run.after.quarantined, vec!["one@example.com".to_string()]);
        assert_eq!(run.after.usable(), vec!["two@example.com"]);
    }

    #[test]
    fn a_machine_with_no_claude_code_records_that_as_why_nothing_was_repaired() {
        let host = a_machine(
            &[("one@example.com", Some(Quarantine::RenewalRejected))],
            &[],
        )
        .with_env("PERCH_CLAUDE_BIN", "/nowhere/claude");

        let (run, said) = a_watched_run(&host, &[]).expect("it runs");

        assert!(
            relogins(&host).is_empty(),
            "`relogin` probes for one before it starts, so there is no login to walk"
        );
        assert_eq!(run.repair.left_quarantined(), vec!["one@example.com"]);
        assert!(
            said.contains("there is no Claude Code to log in with"),
            "so that somebody fixes the right thing: {said}"
        );
    }

    /// `perch relogin` refuses a login that came back as somebody else, and the
    /// Repair reports that refusal rather than re-implementing it: two
    /// implementations of one rule is how they come to disagree.
    #[test]
    fn a_login_as_the_wrong_account_is_reported_as_the_refusal_perch_already_made() {
        let host = a_machine(
            &[("one@example.com", Some(Quarantine::RenewalRejected))],
            &[(
                "one@example.com",
                Login::Leaves(crate::error::EXIT_CONFLICT),
            )],
        );

        let (_, said) = a_watched_run(&host, &[]).expect("it runs");

        assert!(
            said.contains("`perch relogin one@example.com` exited 13"),
            "the exit code is the whole of what the Repair sees: {said}"
        );
        assert!(
            said.contains("that login came back as a different Account"),
            "a bare 13 says nothing to somebody reading the report a week later: {said}"
        );
        assert!(said.contains("not a fault in Perch"), "{said}");
    }

    /// Every exit code `relogin` refuses with is named, so a report read later
    /// says what happened rather than a number — and everything else is Perch
    /// disagreeing with itself, which is a bug.
    #[test]
    fn each_refusal_relogin_makes_is_named_and_anything_else_is_a_fault_in_perch() {
        use crate::error::*;
        for (status, expected) in [
            (EXIT_PROFILE_LIVE, "a client is running against the Profile"),
            (EXIT_NOT_FOUND, "the login did not complete"),
            (EXIT_PROBE_REFUSED, "Claude Code did not report a version"),
            (EXIT_HELD, "another Perch is holding the registry"),
            (EXIT_KEYCHAIN_UNAVAILABLE, "the keychain would not open"),
        ] {
            let (fault, said) = refused_with(status);
            assert_eq!(fault, Fault::Upstream, "exit {status} is a refusal");
            assert_eq!(said, expected);
        }

        // A Cycling code from a command that never Cycles, and Perch's own
        // Quarantined refusal from the one command that ends a Quarantine.
        for status in [EXIT_NO_CANDIDATE, EXIT_QUARANTINED, EXIT_GENERAL] {
            assert_eq!(refused_with(status).0, Fault::Perch, "exit {status}");
        }
    }

    /// `relogin` refuses when the Account was removed while the login was
    /// happening. Reading "is it Quarantined?" off the listing alone would call
    /// that a repair, and the run would go on to claim it had proved something
    /// about an Account that is gone.
    #[test]
    fn an_account_removed_while_its_login_was_happening_is_not_reported_repaired() {
        let host = a_machine(
            &[
                ("one@example.com", Some(Quarantine::RenewalRejected)),
                ("two@example.com", None),
            ],
            &[("one@example.com", Login::Removes)],
        );

        let (run, said) = a_watched_run(&host, &[]).expect("a Repair cannot stop a run");

        let Repair::Found(attempts) = &run.repair else {
            panic!("one Account was Quarantined: {:?}", run.repair);
        };
        assert_eq!(
            attempts[0].ended,
            Ending::NoLongerListed,
            "gone is not cleared"
        );
        assert!(
            said.contains("Perch does not list it at all any more, so nothing was repaired"),
            "{said}"
        );
        assert!(
            !said.contains("it is no longer Quarantined"),
            "a removed Account must not read as a repair: {said}"
        );
    }

    /// A Repair that could not read the listing still has something to type.
    #[test]
    fn a_repair_that_could_not_read_the_listing_says_what_would_say_why() {
        let repair = Repair::NotRead("`perch list --json` exited 1".to_string());

        assert_eq!(
            repair.finish_by_hand(),
            vec!["perch list --json".to_string()]
        );
    }

    /// The listing decides whether an Account was repaired, not the exit code —
    /// so a `relogin` claiming it worked over an Account Perch still calls
    /// Quarantined is caught rather than believed.
    #[test]
    fn a_relogin_that_exited_nought_and_repaired_nothing_is_a_fault_in_perch() {
        let host = a_machine(
            &[("one@example.com", Some(Quarantine::RenewalRejected))],
            &[("one@example.com", Login::Leaves(crate::error::EXIT_OK))],
        );

        let (run, said) = a_watched_run(&host, &[]).expect("a Repair cannot stop a run");

        assert_eq!(run.repair.left_quarantined(), vec!["one@example.com"]);
        assert!(said.contains("This is a fault in Perch."), "{said}");
    }

    #[test]
    fn the_report_holds_the_repair_as_its_own_section() {
        let host = a_machine(
            &[
                ("one@example.com", Some(Quarantine::RenewalRejected)),
                ("two@example.com", Some(Quarantine::NoRefreshToken)),
            ],
            &[(
                "two@example.com",
                Login::Leaves(crate::error::EXIT_NOT_FOUND),
            )],
        );
        let phases = [a_phase("runs anywhere", Needs::NOTHING)];

        let (run, _) = a_watched_run(&host, &phases).expect("it runs");
        let written = host
            .read_file(&Path::new("/tmp/reports").join(run.file_name()))
            .expect("the report was written");

        assert!(written.contains("## Repair"), "{written}");
        assert!(
            written.contains("- one@example.com was Quarantined: Anthropic would not renew"),
            "{written}"
        );
        assert!(
            written.contains("exited 0 — it is no longer Quarantined"),
            "{written}"
        );
        assert!(
            written.contains("- two@example.com was Quarantined: the Credential Perch holds"),
            "{written}"
        );
        assert!(written.contains("it is Quarantined still"), "{written}");
        assert!(
            written.contains("$ perch relogin two@example.com"),
            "{written}"
        );
        // Both figures, so a run against two Accounts and a run against two
        // Accounts one of which was dead do not read alike.
        assert!(
            written.contains("- This machine can prove 1 of 1 phase"),
            "{written}"
        );
        assert!(
            written.contains("- This machine can now prove 1 of 1 phase"),
            "{written}"
        );
    }

    /// The Repair happens before any phase acts, so no phase is ever pointed at
    /// an Account that was never going to work.
    #[test]
    fn nothing_acts_until_the_repair_has_had_its_turn() {
        fn reads_the_machine(perch: &Perch<'_>) -> Proof {
            let listing = perch.json(&["list", "--json"])?;
            let quarantined: Vec<&serde_json::Value> = listing["accounts"]
                .as_array()
                .expect("a listing")
                .iter()
                .filter(|account| !account["quarantined"].is_null())
                .collect();
            assert!(
                quarantined.is_empty(),
                "a phase ran against a Quarantined Account: {quarantined:?}"
            );
            Ok(vec![
                "nothing was Quarantined by the time this ran".to_string(),
            ])
        }

        let host = a_machine(
            &[("one@example.com", Some(Quarantine::RenewalRejected))],
            &[],
        );

        let (run, _) = a_watched_run(
            &host,
            &[Phase {
                name: "reads the machine",
                needs: Needs::AN_ACCOUNT,
                prove: reads_the_machine,
            }],
        )
        .expect("it runs");

        assert!(matches!(run.outcomes[0].1, Outcome::Proved(_)));
    }

    /// The Account somebody is on can be the Quarantined one, and then `perch
    /// status` is about an Account nothing will work as.
    #[test]
    fn an_active_account_the_repair_could_not_clear_is_not_one_a_phase_can_use() {
        // A healthy Account beside the broken one, deliberately: with only the
        // Quarantined one held, the count would refuse this phase on its own and
        // the rule being tested here would never be reached. What has to be
        // proved is that a machine with an Account to spare *still* cannot prove
        // a phase about the Account it is on.
        let host = a_machine(
            &[
                ("one@example.com", Some(Quarantine::RenewalRejected)),
                ("two@example.com", None),
            ],
            &[(
                "one@example.com",
                Login::Leaves(crate::error::EXIT_NOT_FOUND),
            )],
        );

        let (run, _) = a_watched_run(
            &host,
            &[a_phase("needs the active one", Needs::THE_ACTIVE_ACCOUNT)],
        )
        .expect("it runs");

        assert_eq!(run.after.active, Some("one@example.com".to_string()));
        assert_eq!(
            run.after.usable(),
            vec!["two@example.com"],
            "the count this phase asks for is satisfied, and it still cannot run"
        );
        let Outcome::Skipped(why) = &run.outcomes[0].1 else {
            panic!("the Account this machine is on cannot be used: {run:?}");
        };
        assert_eq!(
            why, "one@example.com is the Account this machine is on, and it is still Quarantined",
            "the Account it is about, not the number of Accounts it holds"
        );
    }

    /// A `perch list --json` that could not be read is recorded and carried
    /// past: failing over a Perch that does not work is a phase's job, and the
    /// Repair's job is never to stop the run.
    #[test]
    fn a_listing_the_repair_could_not_read_is_recorded_rather_than_raised() {
        let host = marked(with_a_claude_code(with_a_perch(holding(
            a_bare_machine(),
            &[an_account("one@example.com", None)],
        ))));

        let (run, said) = a_watched_run(&host, &[]).expect("a Repair cannot stop a run");

        assert!(matches!(run.repair, Repair::NotRead(_)), "{:?}", run.repair);
        assert!(said.contains("Nothing could be repaired"), "{said}");
        assert!(said.contains("This is a fault in Perch."), "{said}");
    }

    // ---- the wizard -------------------------------------------------------

    fn set_it_up(host: &dyn Host, args: &SetupArgs) -> Result<(Marker, String)> {
        let mut said = Vec::new();
        let marker = set_up(host, "/build/perch", args, &[], &mut said)?;
        Ok((marker, String::from_utf8(said).unwrap()))
    }

    /// A bare machine with the binary the wizard is about to drive on it. Every
    /// wizard test needs one, because a wizard with nothing to drive is refused
    /// before it does anything else.
    fn with_a_perch(host: FakeHost) -> FakeHost {
        host.with_file("/build/perch", "")
    }

    /// `cargo run --bin dogfood-setup` builds the wizard and not the binary it
    /// drives, so this is the first thing somebody meets. Said before the
    /// passphrase rather than after it.
    #[test]
    fn a_wizard_with_no_perch_to_drive_says_how_to_build_one() {
        let refused = set_it_up(&a_bare_machine(), &SetupArgs::default())
            .expect_err("there is nothing at /build/perch");

        assert!(refused.to_string().contains("cargo build"), "{refused}");
        assert!(refused.to_string().contains(BIN_VARIABLE), "{refused}");
    }

    /// The Export is the whole of what a marker stands for, so the one setup
    /// that may skip it is the one with nothing to save. Anywhere else an
    /// unattended run is refused rather than quietly marking the machine.
    #[test]
    fn an_unattended_setup_is_refused_on_a_machine_that_holds_something_to_lose() {
        let bare = with_a_perch(a_bare_machine());
        let (wrote, said) = set_it_up(
            &bare,
            &SetupArgs {
                unattended: true,
                ..SetupArgs::default()
            },
        )
        .expect("nothing here is at risk");
        assert_eq!(wrote.export, Held::NothingHeld);
        assert!(said.contains("Unattended"), "{said}");

        let logged_in = with_a_perch(holding(
            a_bare_machine(),
            &[an_account("one@example.com", None)],
        ));
        let refused = set_it_up(
            &logged_in,
            &SetupArgs {
                unattended: true,
                ..SetupArgs::default()
            },
        )
        .expect_err("this machine has an Account to lose");

        assert!(refused.to_string().contains("--unattended"), "{refused}");
        assert!(marker(&logged_in).is_err(), "a refused setup marks nothing");
    }

    /// The order the guarantee needs: the Export runs, and only then is anything
    /// written down. An Export that failed leaves the machine unmarked.
    #[test]
    fn a_machine_holding_accounts_is_exported_before_it_is_marked() {
        let host = with_a_perch(holding(
            a_bare_machine(),
            &[an_account("one@example.com", None)],
        ));
        let ran = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = ran.clone();
        let host = host.with_login(move |_, _| {
            seen.set(true);
            0
        });

        let (wrote, said) = set_it_up(
            &host,
            &SetupArgs {
                export_to: Some(PathBuf::from("/tmp/perch.age")),
                ..SetupArgs::default()
            },
        )
        .expect("the Export worked");

        assert!(ran.get(), "`perch export` was never run");
        assert_eq!(
            wrote.export,
            Held::Exported {
                path: PathBuf::from("/tmp/perch.age")
            }
        );
        assert_eq!(marker(&host).expect("and it was written"), wrote);
        assert!(said.contains("An Export comes before anything else"));
        assert!(
            host.effects().iter().any(|effect| matches!(
                effect,
                crate::host::fake::Effect::ExecInteractive { args, .. }
                    if args == &["export".to_string(), "/tmp/perch.age".to_string()]
            )),
            "the wizard hands the terminal to `perch export`, where the \
             passphrase is typed: {:?}",
            host.effects()
        );
    }

    #[test]
    fn an_export_that_failed_leaves_the_machine_unmarked() {
        let host = with_a_perch(holding(
            a_bare_machine(),
            &[an_account("one@example.com", None)],
        ))
        .with_login(|_, _| crate::error::EXIT_INVALID);

        let refused = set_it_up(
            &host,
            &SetupArgs {
                export_to: Some(PathBuf::from("/tmp/perch.age")),
                ..SetupArgs::default()
            },
        )
        .expect_err("the Export did not happen");

        assert!(refused.to_string().contains("perch export"), "{refused}");
        assert!(
            marker(&host).is_err(),
            "the marker is the Export's receipt, and there was no Export"
        );
    }

    /// A machine with nothing on it has nothing an Export could save, so the
    /// login comes first — and the Export follows only if one arrived.
    #[test]
    fn a_login_declined_marks_the_machine_as_holding_nothing() {
        let host = with_a_perch(a_bare_machine()).with_answers(&["n"]);

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("nothing was at risk");

        assert_eq!(wrote.export, Held::NothingHeld);
        assert!(said.contains("No login walked."));
        assert_eq!(marker(&host).expect("still marked"), wrote);
    }

    #[test]
    fn a_login_that_added_nothing_is_said_rather_than_exported_over() {
        let host = with_a_perch(a_bare_machine()).with_answers(&["y"]);

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("an abandoned login");

        assert_eq!(wrote.export, Held::NothingHeld);
        assert!(said.contains("The login added no Account."));
    }

    #[test]
    fn a_run_that_stopped_is_the_one_worth_having_written_down() {
        fn stops(_: &Perch<'_>) -> Proof {
            Err(Setback::upstream("Anthropic was slow"))
        }

        let host = marked(a_bare_machine());
        let phases = [Phase {
            name: "stops",
            needs: Needs::NOTHING,
            prove: stops,
        }];

        let run = a_run(&host, &phases).expect("a marked machine runs");

        let written = host
            .read_file(&Path::new("/tmp/reports").join(run.file_name()))
            .expect("a stopped run is still reported");
        assert!(written.contains("### stops — stopped"));
        assert!(written.contains("Anthropic was slow"));
    }
}
