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
    /// Reported and not yet acted on: the Repair itself is #125. Until it lands,
    /// [`Needs::accounts`] counts a Quarantined Account as held, which is
    /// harmless only for as long as no phase reads a Credential.
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
    /// How many Accounts Perch must hold, at least.
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

    /// A phase that reads what Perch holds, and so needs it to hold something.
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

    /// How many of these phases this machine can prove, as it stands.
    pub fn provable(&self, phases: &[Phase]) -> usize {
        phases
            .iter()
            .filter(|phase| self.unmet(&phase.needs).is_none())
            .count()
    }

    /// The first thing this machine has not got that a phase needs, said as the
    /// line the skip is printed with — or nothing, if it can prove it.
    ///
    /// First rather than all of them: a skip line is read to find out what to
    /// fix next, and a machine missing three things is fixed one at a time.
    pub fn unmet(&self, needs: &Needs) -> Option<String> {
        if self.accounts.len() < needs.accounts {
            return Some(format!(
                "Perch holds {} here and this phase needs {}",
                crate::commands::accounts(self.accounts.len()),
                crate::commands::accounts(needs.accounts)
            ));
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
        if needs.active && self.active.is_none() {
            return Some(
                "no Account is active here, so there is no Account to be about".to_string(),
            );
        }
        None
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
            // The figure, and the whole reason the Preflight is said out loud.
            format!(
                "This machine can prove {} of {}",
                self.provable(phases),
                counted(phases.len())
            ),
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
            return Err(Setback::perch(format!(
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
        Setback {
            fault: Fault::Perch,
            because: because.into(),
            now_true: Vec::new(),
            put_it_back: Vec::new(),
        }
    }

    /// News about something upstream, with nothing changed on the machine.
    pub fn upstream(because: impl Into<String>) -> Setback {
        Setback {
            fault: Fault::Upstream,
            ..Setback::perch(because)
        }
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
            match self.fault {
                Fault::Perch => "This is a fault in Perch.".to_string(),
                Fault::Upstream => {
                    "This is news about something upstream, not a fault in Perch.".to_string()
                }
            },
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
            lines.push("What puts it back:".to_string());
            lines.extend(self.put_it_back.iter().map(|line| format!("  $ {line}")));
        }

        lines.join("\n")
    }
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
    pub preflight: Preflight,
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
/// phases, one at a time, stopping at the first that does — and the report,
/// written whatever happened, because a run that stopped is the one worth
/// having written down.
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

    let mut outcomes: Vec<(&'static str, Outcome)> = Vec::new();
    let mut stopped = false;
    for phase in phases {
        let outcome = if stopped {
            Outcome::NotRun
        } else if let Some(why) = preflight.unmet(&phase.needs) {
            Outcome::Skipped(why)
        } else {
            match (phase.prove)(&perch) {
                Ok(established) => Outcome::Proved(established),
                Err(setback) => {
                    stopped = true;
                    Outcome::Stopped(setback)
                }
            }
        };

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
    use crate::host::FakeHost;
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
