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

/// The version this build writes, and the only one it reads.
///
/// Two rather than one because the marker now records what the machine was
/// arranged to prove as well as what was saved, and *other* is refused rather
/// than only *newer*: Perch has no installed base, so a marker written before
/// the arrangement existed is one minute of the wizard's time to replace and no
/// lines at all of code that has to reason about what it left out.
pub const MARKER_VERSION: u32 = 2;

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
    /// What this machine was deliberately set up to prove.
    pub arrangement: Arrangement,
}

/// What the wizard was told, beyond what it could see for itself.
///
/// Both of these are things only a person knows, and both gate phases — so both
/// are asked for once, while somebody is watching, rather than guessed at on
/// every run. A machine nobody told anything is [`Arrangement::default`], and
/// the phases needing either simply skip there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrangement {
    /// The Group set aside for the phases that need a pair of Accounts.
    ///
    /// Named rather than discovered. A Group is a declaration somebody made
    /// (`CONTEXT.md`), so a phase that Cycled inside the largest Group it could
    /// find would be moving Accounts around a set declared for somebody's own
    /// reasons — and doing it after the marker check had passed, which is the
    /// one place the suite promises not to surprise anybody.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// How many logins with Anthropic this person holds, in total, across every
    /// machine.
    ///
    /// The one fact that makes "this machine is behind" computable: a machine
    /// holding fewer Accounts than there are logins has one spare to `add`, and
    /// that is the only circumstance in which a Dogfood run may walk a real
    /// browser login for an Account it did not already have.
    #[serde(default)]
    pub logins: usize,
}

impl Arrangement {
    /// The line the wizard and a report both say it with.
    pub fn said(&self) -> String {
        let pair = match &self.group {
            Some(group) => {
                format!("the Group `{group}` is set aside for the phases needing a pair")
            }
            None => "no Group is set aside".to_string(),
        };
        format!("{pair}, and {} held in all", logins(self.logins))
    }
}

/// A count of logins, with the noun agreeing with it. The rule
/// [`crate::commands::accounts`] holds for Accounts, and for the same reason.
fn logins(held: usize) -> String {
    match held {
        1 => "1 login".to_string(),
        _ => format!("{held} logins"),
    }
}

/// What the wizard's first act came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Held {
    /// An Export was written here, and every Account on this machine at that
    /// moment is in it — which is what `accounts` names.
    ///
    /// The addresses travel because the guarantee is about a *set* rather than
    /// about a file. A marker saying only "there is an Export at this path" is
    /// true for ever, including three `perch add`s later, and the run it waves
    /// through is the one whose Credentials nothing has a copy of.
    Exported {
        path: PathBuf,
        accounts: Vec<String>,
    },
    /// Perch held no Accounts, so there was nothing an Export could have saved.
    /// The only machine this is honest on is one that has never logged in — a
    /// CI runner, or a laptop somebody has just cloned the repository onto.
    NothingHeld,
}

impl Held {
    /// The line a report and the Preflight both say it with.
    pub fn said(&self) -> String {
        match self {
            Held::Exported { path, .. } => format!("Export at {}", path.display()),
            Held::NothingHeld => "no Export: this machine held no Accounts".to_string(),
        }
    }

    /// Which Accounts this is a receipt for. `NothingHeld` is a receipt for
    /// none, which is the honest reading of it rather than a special case.
    pub fn covers(&self) -> &[String] {
        match self {
            Held::Exported { accounts, .. } => accounts,
            Held::NothingHeld => &[],
        }
    }
}

/// Refuses a machine holding an Account the marker's Export was not taken over.
///
/// The marker is time-of-setup and nothing was revisiting it, so the ordinary
/// sequence — mark a fresh machine, `perch add` three times over the following
/// weeks, run the suite — passed the guard and then moved real Credentials
/// around with no Export behind them at all, while the report said "no Export:
/// this machine held no Accounts".
///
/// That is exactly the belief ADR 0037 names as the one that is wrong on the
/// occasion it matters. The Export is only a safety net if it is guaranteed,
/// and a guarantee about a set has to be checked against the set.
fn refuse_accounts_no_export_covers(marker: &Marker, held: &[String]) -> Result<()> {
    let covered = marker.export.covers();
    let uncovered: Vec<&str> = held
        .iter()
        .filter(|email| {
            !covered
                .iter()
                .any(|known| registry::same_name(known, email))
        })
        .map(String::as_str)
        .collect();
    if uncovered.is_empty() {
        return Ok(());
    }

    Err(PerchError::Invalid(format!(
        "This machine was marked when there was {}, and it now holds {} that no \
         Export covers: {}.\n\
         A Dogfood run moves real Credentials around, and the Export is the only \
         thing that makes that reversible — so the suite will not touch a machine \
         holding a login nothing has a copy of.\n\
         {HOW_TO_SET_UP}",
        marker.export.said(),
        crate::commands::accounts(uncovered.len()),
        uncovered.join(", "),
    )))
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
    //
    // Any version but this one is refused, and the two are refused apart. A
    // newer Perch's marker is the standing forward-looking guard and says so in
    // the words every other one of Perch's files uses. An older one earns a
    // sentence of its own because the answer to it is different and is a minute
    // of somebody's time: a marker written before the arrangement existed says
    // nothing about which Group was set aside or how many logins are held, and
    // a run reading it would skip half the suite while reporting that the
    // machine simply had not got the Accounts.
    match crate::error::claimed_version(&contents) {
        Some(version) if version > MARKER_VERSION => {
            return Err(crate::error::written_by_a_newer_perch(
                &path.display().to_string(),
                "dogfood marker",
                version,
                MARKER_VERSION,
            ));
        }
        Some(version) if version != MARKER_VERSION => {
            return Err(PerchError::Invalid(format!(
                "This machine was marked by an earlier Perch, whose marker \
                 (version {version}) says nothing about what the machine was \
                 set up to prove.\n{HOW_TO_SET_UP}"
            )));
        }
        _ => {}
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
    /// Which Group each Account is in, where it is in one.
    ///
    /// Kept beside the Accounts rather than derived when a phase asks, because
    /// the question the arrangement poses — *does this machine still hold two
    /// usable Accounts in the Group somebody set aside?* — is one about
    /// membership, and membership changes under a run as surely as a Quarantine
    /// does.
    pub in_group: Vec<(String, String)>,
    /// What the marker says this machine was set up to prove.
    ///
    /// Not read by [`Preflight::taken`], which describes what is *here*: the
    /// wizard takes a Preflight before it has decided the arrangement, and a
    /// machine describing itself must not need a marker that does not exist
    /// yet. A run sets it from the marker before anything is measured.
    pub arrangement: Arrangement,
    /// Whether somebody is at the terminal, having said so and been believed.
    ///
    /// Both halves, and neither alone (ADR 0038): the variable is what opts in,
    /// and the terminal is what makes the opting-in honourable.
    pub attended: bool,
    /// Why this machine has no service manager Perch could drive, where it has
    /// not.
    ///
    /// Asked of the machine rather than inferred from the platform, which is the
    /// Preflight's whole discipline: a Linux container with no session bus has a
    /// `systemctl` on `PATH` that answers nothing, and a run that counted it in
    /// would fail a Service phase over the machine rather than over Perch (ADR
    /// 0037).
    pub no_service_manager: Option<String>,
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
    /// How many usable Accounts the *arranged* Group must hold.
    ///
    /// The arranged one and not the largest one: Cycling only ever happens
    /// inside a Group, so a phase about it moves Accounts around a declaration
    /// somebody made, and the only declaration this suite may act on is the one
    /// the wizard was told to act on.
    pub grouped: usize,
    /// A Claude Code to launch.
    pub client: bool,
    /// A network that answers.
    pub network: bool,
    /// One of those Accounts to be active.
    pub active: bool,
    /// Somebody at the terminal to walk a login or quit a client.
    ///
    /// What a person may be asked for is an act and never a verdict (ADR 0038)
    /// — this says the phase needs one of them done, not that it needs one
    /// witnessed.
    pub attended: bool,
    /// A login this machine has not got: more logins held than Accounts here.
    ///
    /// The only circumstance in which a run may walk a browser login for an
    /// Account it did not already have, and the reason it is a *machine* being
    /// behind rather than a person having a spare — across a matrix of four
    /// machines, the one that can prove `add` is the one that is out of date.
    pub spare_login: bool,
    /// A service manager Perch can drive — launchd, `systemd --user`, or the
    /// Windows task scheduler.
    ///
    /// Its own need rather than a platform check, because having one is not the
    /// same as being on a platform that usually does: a container with no
    /// session bus and a `systemd --user` that will not answer is exactly the
    /// machine a Service phase must be counted *out* of rather than fail on
    /// (ADR 0037 — a run proves only what the machine it is on holds).
    pub service_manager: bool,
}

impl Needs {
    /// A phase that runs anywhere — argv, a refusal, an exit code.
    pub const NOTHING: Needs = Needs {
        accounts: 0,
        grouped: 0,
        client: false,
        network: false,
        active: false,
        attended: false,
        spare_login: false,
        service_manager: false,
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

    /// A phase that moves between two Accounts, whether or not they are
    /// declared interchangeable.
    pub const TWO_ACCOUNTS: Needs = Needs {
        accounts: 2,
        active: true,
        ..Needs::NOTHING
    };

    /// A phase about Cycling, which only happens inside a Group.
    pub const A_PAIR: Needs = Needs {
        grouped: 2,
        ..Needs::TWO_ACCOUNTS
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
            in_group: Vec::new(),
            arrangement: Arrangement::default(),
            attended: false,
            no_service_manager: why_no_service_manager(host),
        };
        preflight.read_the_registry(host)?;
        Ok(preflight)
    }

    /// What the marker says this machine was set up to prove, and whether
    /// anybody is watching it happen.
    ///
    /// Apart from [`Preflight::taken`] because the wizard needs the description
    /// before there is a marker to describe it with — and because attendance is
    /// a question about the process the suite is running in rather than about
    /// the machine, which is the one thing a Preflight otherwise never asks.
    pub fn arranged(&mut self, marker: &Marker, attended: bool) {
        self.arrangement = marker.arrangement.clone();
        self.attended = attended;
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
        self.in_group = registry
            .accounts
            .iter()
            .filter_map(|account| Some((account.email().to_string(), account.group.clone()?)))
            .collect();
        Ok(())
    }

    /// The usable Accounts in the Group the wizard set aside, or none at all
    /// where nothing was set aside.
    ///
    /// A machine with no arrangement answers the same as one whose arranged
    /// Group has emptied out, and deliberately: both are machines that cannot
    /// prove anything about Cycling, and the skip line is where the difference
    /// between them is worth spelling out rather than here.
    pub fn arranged_pair(&self) -> Vec<&String> {
        let Some(group) = &self.arrangement.group else {
            return Vec::new();
        };
        let usable = self.usable();
        self.in_group
            .iter()
            .filter(|(_, theirs)| theirs == group)
            .filter_map(|(email, _)| usable.iter().find(|held| **held == email).copied())
            .collect()
    }

    /// Whether this machine has a login it has not got an Account for.
    ///
    /// Arithmetic rather than a question, and the whole of what makes an `add`
    /// phase safe: it runs on the machine that is behind and nowhere else.
    pub fn has_a_spare_login(&self) -> bool {
        self.arrangement.logins > self.accounts.len()
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

    /// *Up to*, because a phase may discover as it runs that this machine can
    /// prove nothing with it (ADR 0038) — a Renewal against a token that has
    /// not run out is the case, and its gate is on the Credential rather than
    /// anywhere the Preflight can cheaply look. The figure is an upper bound and
    /// says so, which is a different thing from a run that quietly proved less.
    fn figure_said(&self, now: &str, phases: &[Phase]) -> String {
        format!(
            "This machine can {now}prove up to {} of {}",
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
        // Attendance first, ahead of everything the machine holds. Every
        // attended phase also needs Accounts, so a CI runner would otherwise
        // report each of them as short of Accounts — true, and not the thing
        // the reader is short of. What is missing there is a person, and no
        // amount of logging in fixes it.
        if needs.attended && !self.attended {
            return Some(format!(
                "nobody is at the terminal: this phase hands one over, and a run \
                 takes those on only where {ATTENDED_VARIABLE} is set"
            ));
        }
        let usable = self.usable();
        if usable.len() < needs.accounts {
            return Some(self.too_few_usable(usable.len(), needs.accounts));
        }
        if needs.grouped > 0 {
            let pair = self.arranged_pair();
            if pair.len() < needs.grouped {
                return Some(self.too_few_arranged(pair.len(), needs.grouped));
            }
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
        if needs.service_manager
            && let Some(why) = &self.no_service_manager
        {
            return Some(why.clone());
        }
        if needs.spare_login && !self.has_a_spare_login() {
            return Some(format!(
                "this machine already holds {}, and the wizard was told there are \
                 {} — so there is no login here left to add",
                crate::commands::accounts(self.accounts.len()),
                logins(self.arrangement.logins),
            ));
        }
        None
    }

    /// Why a phase about Cycling cannot run here.
    ///
    /// Three readings, because "the Group has not got two" is the least likely
    /// of them and the other two send somebody to completely different places:
    /// a machine nobody arranged is a wizard to re-run, and a Group that has
    /// emptied out is a Quarantine or a Remove to look into.
    fn too_few_arranged(&self, usable: usize, needed: usize) -> String {
        let Some(group) = &self.arrangement.group else {
            return format!(
                "no Group is set aside on this machine, and this phase Cycles \
                 within one — {HOW_TO_SET_UP}"
            );
        };
        format!(
            "the Group `{group}` holds {} this machine can use and this phase \
             needs {}",
            crate::commands::accounts(usable),
            crate::commands::accounts(needed),
        )
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
            // What somebody told the wizard, said back to them: the two facts
            // gating a third of the suite are the two facts nothing on the
            // machine can check, so a run that skips those phases has to make
            // it possible to see *why* without opening the marker.
            format!("Arranged: {}", self.arrangement.said()),
            format!(
                "Attended: {}",
                match self.attended {
                    true => "yes — phases that hand over the terminal will run",
                    false => "no — phases that hand over the terminal will skip",
                }
            ),
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
/// Why this machine has no service manager Perch could drive, or `None` where it
/// has one.
///
/// The question a Service phase is counted by, and it is asked by *running* the
/// thing that would have to answer rather than by reading the platform.
/// `systemctl --user` on a box with no logind session, and `launchctl` inside a
/// runner with no GUI domain, both exist on `PATH` and both refuse — so a
/// Preflight that read the platform would count the phase in and then fail it
/// over the machine rather than over Perch (ADR 0037).
///
/// Asked with the harmless half of the interface. `is-active` on a unit that is
/// not installed, and `print` on a label that is not loaded, are questions that
/// change nothing whatever they answer; what is read is whether the service
/// manager was reachable at all, which is a different thing from what it said.
/// So a non-zero exit is fine and only a program that would not run at all is
/// not.
fn why_no_service_manager(host: &dyn Host) -> Option<String> {
    let asking = crate::service::asking(host.platform(), host.user_id())?;
    let args: Vec<&str> = asking.args.iter().map(String::as_str).collect();
    match host.exec(&asking.program, &args) {
        Ok(_) => None,
        Err(why) => Some(format!(
            "`{}` would not run here, so this machine has no service manager              Perch could install a Service into: {why}",
            asking.program,
        )),
    }
}

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

/// How somebody says they are sitting there, so the phases that hand the
/// terminal over may run (ADR 0038).
///
/// A variable rather than an inference from the terminal, and checked against
/// the terminal rather than believed on its own. A tty is not a person — a pane
/// left open upstairs has one — so being there is a claim only somebody there
/// can make; and a variable exported into a CI job or a shell profile reaches
/// contexts where nothing can be typed, so the claim is checked before it is
/// acted on.
pub const ATTENDED_VARIABLE: &str = "PERCH_DOGFOOD_ATTENDED";

/// Which phases to run, as words any part of a phase's name may contain.
///
/// For the Tuesday when one phase failed on Windows and it is that phase that
/// wants running again, rather than the hour of them in front of it. Empty or
/// unset is every phase, which is what an unfiltered run has to keep meaning.
pub const PHASES_VARIABLE: &str = "PERCH_DOGFOOD_PHASES";

/// Whether the phases that hand the terminal over may run.
///
/// Both halves have to agree, and they fail differently on purpose: an
/// unattended run simply skips them, and a run that asked for them on a
/// terminal that cannot carry a question is refused outright — see
/// [`refuse_an_attendance_nobody_could_honour`].
fn attendance(host: &dyn Host) -> bool {
    asked_to_attend(host) && host.is_interactive()
}

fn asked_to_attend(host: &dyn Host) -> bool {
    host.env_var(ATTENDED_VARIABLE)
        .is_some_and(|set| !set.is_empty() && set != "0")
}

/// Refuses an opt-in the terminal could not honour, at the top of the run.
///
/// The failure this prevents is not a skip. A phase that handed the terminal
/// over where nothing could be typed would block on an answer that is never
/// coming — a hang, halfway through a suite, in a pipeline or a CI job where
/// the question is at best on a screen nobody is reading.
fn refuse_an_attendance_nobody_could_honour(host: &dyn Host) -> Result<()> {
    if !asked_to_attend(host) || host.is_interactive() {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "{ATTENDED_VARIABLE} is set, and this is not a terminal anything could \
         be typed at.\n\
         The phases it turns on hand the terminal to a browser login or to a \
         client, so a run that took them on here would stop at the first \
         question and wait for an answer that is not coming. Unset it, or run \
         the suite where you can type."
    )))
}

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

    /// The machine underneath, for the little a phase has to look at directly.
    ///
    /// Not a way round "a phase reads `list --json`, `status --json` and exit
    /// codes" (ADR 0037), which is about not building a backdoor into the
    /// *shipped* binary. What this is for is the same thing the Preflight reads
    /// the registry for: seeing the machine as a person would, with the library
    /// this suite already links, so that what Perch *did* to a Profile can be
    /// checked rather than taken on trust from Perch.
    pub fn host(&self) -> &dyn Host {
        self.host
    }

    /// Puts a question to whoever is watching, and hands back the word they
    /// typed, folded and trimmed.
    ///
    /// Only ever to ask for an *act* — walk this login, quit that client. A
    /// phase does not ask whether it passed (ADR 0038): a verdict that is a
    /// keystroke is one somebody types away on the fourth round trip of the
    /// evening, and nearly everything worth judging is on disk by then anyway.
    ///
    /// Reachable only from a phase the Preflight admitted as attended, so end
    /// of input here is a terminal that went away mid-run rather than a
    /// question asked where none could be.
    pub fn ask(&self, out: &mut dyn Write, question: &str) -> std::result::Result<String, Halt> {
        crate::commands::ask_a_word(self.host, out, question)
            .map_err(|err| {
                Fault::Upstream.because(format!("the terminal could not be read: {err}"))
            })?
            .ok_or_else(|| {
                Halt::Stopped(Fault::Upstream.because(
                    "the terminal ended while this phase was waiting for an answer".to_string(),
                ))
            })
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

/// What a Repair heads its leftovers with, in one place because the run says it
/// on the terminal and the report says it again on disk.
const FINISH_BY_HAND: &str = "What would finish the job by hand:";

/// Commands somebody could type, said the same way wherever a run offers them —
/// a stopped phase's way back, and a Repair's way to finish by hand.
///
/// One place, because a run offers these twice and three spellings of one shape
/// had already grown between two: the heading and the `$` are what somebody
/// scans for, and they should not move depending on which of the two they are
/// reading.
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
    ///
    /// Carries whose fault it was, like every other verdict here. It used to
    /// drop the `Setback`'s `Fault` and say `Fault::Perch` regardless — but
    /// `read_as` classifies a held registry, a live Profile and an unavailable
    /// keychain as `Fault::Upstream` on purpose, because a machine somebody
    /// works on runs other Perches. A `perch watch` holding the lock for the
    /// second the Repair opened in was reported as a bug in Perch, which is the
    /// misclassification ADR 0037 names as the one that gets a suite's red
    /// ignored within a month.
    NotRead { fault: Fault, why: String },
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
            Repair::NotRead { fault, why } => {
                vec![format!(
                    "Nothing could be repaired: {why}. {}",
                    fault.said()
                )]
            }
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
        if let Repair::NotRead { .. } = self {
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
        Err(setback) => {
            return Ok(Repair::NotRead {
                fault: setback.fault,
                why: setback.because,
            });
        }
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
            preflight.attended,
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
    attended: bool,
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

    // A Repair is a browser round trip, so it is an attended act and asks the
    // same question every attended phase asks (ADR 0038). Asked here rather
    // than left to the Phases' own guard, because the Repair runs ahead of
    // them: an opt-in carried in from a cron job's environment, or a run with
    // its output piped, would otherwise open `claude /login` where nothing can
    // be typed and block there for ever. Attendance is never assumed, and a
    // Repair is the one part of a run that would have assumed it.
    if !attended {
        return Ok(attempted(
            None,
            Ending::StillQuarantined {
                fault: Fault::Upstream,
                why: format!(
                    "nobody is at the terminal, and a Repair is a browser round \
                     trip — a run walks those only where {ATTENDED_VARIABLE} is set"
                ),
            },
        ));
    }

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
/// The two ways a phase ends other than by proving something.
///
/// On the error side rather than the success side, and that is the whole of why
/// this type exists: a phase discovering it can prove nothing here is an early
/// return, which is what `?` spells. A Renewal reads one Credential and stops if
/// the token has not run out; on the success side that is a `match` at every
/// call site that could learn the same thing, and one of them eventually
/// forgets.
///
/// A [`Halt::Skipped`] is not a failure. It becomes the [`Outcome::Skipped`] the
/// report already knows how to print, so nothing downstream needs a word for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// This machine cannot prove this, discovered as the phase ran rather than
    /// before it (ADR 0038). Named and counted, never silent.
    Skipped(String),
    /// It stopped, and the run stops with it.
    Stopped(Setback),
}

/// So a phase's ordinary `perch.json(…)?` still reads as it did: what those
/// hand back is a [`Setback`], and a Setback is always a stop.
impl From<Setback> for Halt {
    fn from(setback: Setback) -> Halt {
        Halt::Stopped(setback)
    }
}

impl Halt {
    /// This machine cannot prove this phase, for a reason found at run time.
    ///
    /// Worth a constructor of its own so the call reads as what it means where
    /// a phase gives up, rather than as an error being built.
    pub fn not_here(why: impl Into<String>) -> Halt {
        Halt::Skipped(why.into())
    }
}

pub type Proof = std::result::Result<Vec<String>, Halt>;

/// One phase: a name, what it needs of the machine, and the proving.
///
/// `Copy` so a run can be narrowed to the phases somebody asked for
/// ([`PHASES_VARIABLE`]) without every figure, report and count in the module
/// learning to speak in references — a name, a `Needs` and a function pointer
/// are cheaper to copy than to borrow around.
#[derive(Clone, Copy)]
pub struct Phase {
    pub name: &'static str,
    pub needs: Needs,
    /// The writer is the terminal, where a phase was admitted as attended, and
    /// somewhere harmless where it was not — a phase says what it is about to
    /// ask somebody to do, and nothing else prints from inside one.
    pub prove: fn(&Perch<'_>, &Preflight, &mut dyn Write) -> Proof,
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
    /// What somebody narrowed the run to, where they did.
    ///
    /// Kept so the report says it. A report of one phase and a report of a suite
    /// that has shrunk to one phase are the same document otherwise, and the
    /// second is the kind of thing a matrix is read to notice.
    pub filtered: Option<String>,
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
            match &self.filtered {
                Some(asked) => format!("- Narrowed to `{PHASES_VARIABLE}={asked}`"),
                None => "- Every phase, unfiltered".to_string(),
            },
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
fn prove(perch: &Perch<'_>, standing: &Preflight, phase: &Phase, out: &mut dyn Write) -> Outcome {
    if let Some(why) = standing.unmet(&phase.needs) {
        return Outcome::Skipped(why);
    }
    match (phase.prove)(perch, standing, out) {
        Ok(established) => Outcome::Proved(established),
        // Both are the phase saying it proved nothing. They are told apart by
        // whose problem that is, which is the distinction the whole suite turns
        // on: a Skip is a machine that has not got what the phase needed, and a
        // Setback is somebody's work.
        Err(Halt::Skipped(why)) => Outcome::Skipped(why),
        Err(Halt::Stopped(setback)) => Outcome::Stopped(setback),
    }
}

/// The phases this run is about: all of them, or the ones somebody named.
///
/// The narrowing happens once and everything downstream is measured against the
/// result — the figure, the report, the count. A run filtered to one phase that
/// went on saying "1 of 9" would be describing a suite nobody asked it to run.
fn chosen(host: &dyn Host, phases: &[Phase]) -> (Vec<Phase>, Option<String>) {
    let Some(asked) = host
        .env_var(PHASES_VARIABLE)
        .filter(|set| !set.trim().is_empty())
    else {
        return (phases.to_vec(), None);
    };
    let words: Vec<String> = asked
        .split(',')
        .map(|word| word.trim().to_lowercase())
        .filter(|word| !word.is_empty())
        .collect();

    let narrowed = phases
        .iter()
        .filter(|phase| {
            let name = phase.name.to_lowercase();
            words.iter().any(|word| name.contains(word))
        })
        .copied()
        .collect();
    (narrowed, Some(asked))
}

/// Refuses a narrowing that matched no phase at all.
///
/// A typo in the variable is the whole of what this is about, and it fails in
/// the direction the Preflight figure exists to prevent: a run of nothing
/// reports a green "0 of 0 phases", which is the same document a run that
/// proved everything would produce if the suite were empty. Left alone, the
/// answer to "did Windows pass?" would be yes.
fn refuse_a_narrowing_that_matched_nothing(
    asked: &Option<String>,
    narrowed: &[Phase],
    phases: &[Phase],
) -> Result<()> {
    let Some(asked) = asked else {
        return Ok(());
    };
    if !narrowed.is_empty() {
        return Ok(());
    }
    Err(PerchError::NotFound(format!(
        "{PHASES_VARIABLE}={asked} matches none of the {}, so this run would \
         prove nothing and report it as a run with nothing left to prove.\n\
         Any part of a phase's name will do. They are:\n{}",
        counted(phases.len()),
        phases
            .iter()
            .map(|phase| format!("  {}", phase.name))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}

pub fn dogfood(
    host: &dyn Host,
    built: &str,
    phases: &[Phase],
    reports: &Path,
    out: &mut dyn Write,
) -> Result<Run> {
    let marker = marker(host)?;
    // Before anything is described, let alone acted on: an opt-in the terminal
    // could not honour is a hang halfway through rather than a skip, and the
    // only harmless moment to say so is before the run has touched anything.
    refuse_an_attendance_nobody_could_honour(host)?;
    // Read before the phases rather than after them, so a report is named after
    // when the run started. A full run is somebody sitting there for an hour,
    // and a stamp taken at the end names the wrong sitting.
    let began = host.now();
    let mut preflight = Preflight::taken(host)?;
    preflight.arranged(&marker, attendance(host));
    // The second half of the marker's promise, and the half nothing was asking.
    // Before the Repair, because the Repair is the first thing that hands the
    // terminal to a real login.
    refuse_accounts_no_export_covers(&marker, &preflight.accounts)?;
    let perch = Perch::under_test(host, built);

    // Narrowed before the figure is said, so every count in the run and in the
    // report is about the same set of phases.
    let (narrowed, filtered) = chosen(host, phases);
    refuse_a_narrowing_that_matched_nothing(&filtered, &narrowed, phases)?;
    let phases = narrowed.as_slice();

    say(out, &format!("Under test: {}", perch.bin().display()))?;
    if let Some(asked) = &filtered {
        say(
            out,
            &format!(
                "Narrowed to {} by {PHASES_VARIABLE}={asked}",
                counted(phases.len())
            ),
        )?;
    }
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
            prove(&perch, &standing, phase, out)
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
        filtered,
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
///
/// `sources` is the tree `built` was compiled from — `CARGO_MANIFEST_DIR`, and
/// passed in rather than read from the environment here so that the guard using
/// it can be driven against a fake machine. A path that is not there is fine:
/// the guard has nothing to say about a Perch whose source is somewhere else.
pub fn set_up(
    host: &dyn Host,
    built: &str,
    sources: &Path,
    args: &SetupArgs,
    phases: &[Phase],
    out: &mut dyn Write,
) -> Result<Marker> {
    let perch = Perch::under_test(host, built);
    refuse_a_binary_that_is_not_there(host, &perch)?;
    refuse_a_binary_the_source_has_moved_past(host, &perch, sources)?;
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
            accounts: preflight.accounts.clone(),
        }
    };

    let arrangement = if args.unattended {
        // A runner holds nothing and nobody is there to say what it holds
        // elsewhere. Both phases this gates then skip, which is the honest
        // answer and the one CI wants.
        Arrangement::default()
    } else {
        arrange(host, &perch, &mut preflight, out)?
    };

    let marker = Marker {
        version: MARKER_VERSION,
        marked_at: host.now(),
        perch_version: env!("CARGO_PKG_VERSION").to_string(),
        export,
        arrangement,
    };
    mark(host, &marker)?;

    say(out, "")?;
    say(out, &format!("Marked: {}", marker_path(host)?.display()))?;
    say(out, &marker.export.said())?;
    say(out, "")?;
    // The figure, said at the end of setup as well as at the start of a run:
    // whoever just walked this is the person who can do something about it.
    // Measured with the arrangement in force, because arranging one is most of
    // what the figure just changed.
    preflight.arranged(&marker, attendance(host));
    for line in preflight.said(phases) {
        say(out, &line)?;
    }

    Ok(marker)
}

/// The two things only a person knows, asked once, while they are watching.
///
/// Both gate phases and neither is on the machine: how many logins exist in all
/// is a fact about somebody's subscriptions, and which Group may be Cycled
/// through is a decision about their Accounts. A run inferring either would be
/// acting on something nobody told it — and it would be doing so after the
/// marker check had passed, which is the one point in a Dogfood run where the
/// suite promises no surprises.
fn arrange(
    host: &dyn Host,
    perch: &Perch<'_>,
    preflight: &mut Preflight,
    out: &mut dyn Write,
) -> Result<Arrangement> {
    say(out, "")?;
    say(out, "What this machine may be asked to prove:")?;

    let logins = ask_how_many_logins(host, preflight.accounts.len(), out)?;
    let group = set_a_group_aside(host, perch, preflight, out)?;
    Ok(Arrangement { group, logins })
}

/// How many logins there are in all, across every machine.
///
/// What it buys is the `add` phase: a machine holding fewer Accounts than this
/// has a login it has not got, and that is the only circumstance in which a run
/// may walk a browser login for an Account it did not already have. Answered
/// with what is here by anybody who does not want that, which is why the
/// default is the count held rather than a number that turns the phase on.
fn ask_how_many_logins(host: &dyn Host, held: usize, out: &mut dyn Write) -> Result<usize> {
    let answered = crate::commands::ask_a_word(
        host,
        out,
        &format!(
            "  How many logins with Anthropic do you hold in all, across every \
             machine? [{held}] "
        ),
    )?;
    match answered.as_deref().map(str::trim) {
        Some("") | None => Ok(held),
        Some(typed) => match typed.parse::<usize>() {
            // Fewer than are on this machine is not a number a phase could act
            // on — `spare_login` is arithmetic between the two — so it is taken
            // as the count held and said out loud rather than stored to puzzle
            // somebody later.
            Ok(count) if count >= held => Ok(count),
            Ok(count) => {
                say(
                    out,
                    &format!(
                        "  This machine already holds {}, so {count} is taken as \
                         {held}.",
                        crate::commands::accounts(held)
                    ),
                )?;
                Ok(held)
            }
            Err(_) => {
                say(out, &format!("  `{typed}` is not a number; taking {held}."))?;
                Ok(held)
            }
        },
    }
}

/// Which Group the phases that Cycle may move Accounts around in.
///
/// Offered rather than chosen: a Group already holding a usable pair is the
/// obvious answer and is proposed as one, and a machine without such a Group is
/// asked before anything is declared or moved. Declining leaves no Group set
/// aside, which costs the two Cycling phases and nothing else.
fn set_a_group_aside(
    host: &dyn Host,
    perch: &Perch<'_>,
    preflight: &mut Preflight,
    out: &mut dyn Write,
) -> Result<Option<String>> {
    let usable: Vec<String> = preflight.usable().into_iter().cloned().collect();
    if usable.len() < 2 {
        say(
            out,
            "  The phases that Cycle need two usable Accounts here, and this \
             machine has not got them yet. None is set aside.",
        )?;
        return Ok(None);
    }

    if let Some(group) = a_group_already_holding_a_pair(preflight, &usable) {
        let answered = crate::commands::ask_a_word(
            host,
            out,
            &format!("  Set the Group `{group}` aside for the phases that Cycle? [Y/n] "),
        )?;
        if !matches!(answered.as_deref(), Some("n" | "no")) {
            return Ok(Some(group));
        }
        say(out, "  No Group set aside.")?;
        return Ok(None);
    }

    let pair = &usable[..2];
    let answered = crate::commands::ask_a_word(
        host,
        out,
        &format!(
            "  No Group here holds two usable Accounts. Declare `{ARRANGED_GROUP}` \
             and move {} and {} into it? [y/N] ",
            pair[0], pair[1]
        ),
    )?;
    if !matches!(answered.as_deref(), Some("y" | "yes")) {
        say(
            out,
            "  No Group set aside. The phases that Cycle will skip on this machine.",
        )?;
        return Ok(None);
    }

    declare_and_fill(perch, pair, out)?;
    // What was just moved changes who is in which Group, and the figure printed
    // at the end of setup is measured from it.
    preflight.read_the_registry(host)?;
    Ok(Some(ARRANGED_GROUP.to_string()))
}

/// The Group the wizard declares where somebody has none to offer. A fixed name
/// rather than one typed, because it is Perch's suggestion and a name somebody
/// chose is one they would rather have declared themselves.
const ARRANGED_GROUP: &str = "dogfood";

/// A Group on this machine already holding two Accounts a phase could use.
fn a_group_already_holding_a_pair(preflight: &Preflight, usable: &[String]) -> Option<String> {
    let mut counted: Vec<(String, usize)> = Vec::new();
    for (email, group) in &preflight.in_group {
        if !usable.contains(email) {
            continue;
        }
        match counted.iter_mut().find(|(name, _)| name == group) {
            Some((_, held)) => *held += 1,
            None => counted.push((group.clone(), 1)),
        }
    }
    counted
        .into_iter()
        .find(|(_, held)| *held >= 2)
        .map(|(name, _)| name)
}

/// Declares the Group and moves the pair into it, through the binary under test
/// rather than through the registry.
///
/// The wizard is the last place to reimplement something `perch group` already
/// does: a namespace shared with Aliases, a Group that already exists, an
/// Account that will not move — every one of those refusals is already written,
/// and a second implementation of them is how the two come to disagree.
fn declare_and_fill(perch: &Perch<'_>, pair: &[String], out: &mut dyn Write) -> Result<()> {
    let declared = perch
        .run(&["group", "add", ARRANGED_GROUP])
        .map_err(|setback| PerchError::Other(setback.because))?;
    // A Group that is already there is the answer this wanted, arrived at
    // earlier — the pair below is what actually has to be true.
    if !declared.succeeded() && declared.status != crate::error::EXIT_CONFLICT {
        return Err(PerchError::Other(format!(
            "`perch group add {ARRANGED_GROUP}` exited {}: {}",
            declared.status,
            declared.stderr.trim()
        )));
    }

    for email in pair {
        let moved = perch
            .run(&["group", "move", email, ARRANGED_GROUP])
            .map_err(|setback| PerchError::Other(setback.because))?;
        if !moved.succeeded() {
            return Err(PerchError::Other(format!(
                "`perch group move {email} {ARRANGED_GROUP}` exited {}: {}",
                moved.status,
                moved.stderr.trim()
            )));
        }
        say(out, &format!("  {email} is now in `{ARRANGED_GROUP}`."))?;
    }
    Ok(())
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
         drives. Run {HOW_TO_BUILD} first, or point {BIN_VARIABLE} at an \
         installed Perch.",
        bin.display()
    )))
}

/// The command that builds the binary the wizard drives, said in both refusals
/// about not having a usable one. One string, for the reason [`HOW_TO_SET_UP`]
/// is one: the two places printing it must not come to disagree about it.
const HOW_TO_BUILD: &str = "`cargo build --features dogfood --bins`";

/// What a binary is built from, as far as a walk can see it: the source tree,
/// and the two files deciding what goes into it. A dependency bump changes the
/// binary as surely as an edit does, and costs one `modified_at` to notice.
const BUILT_FROM: &[&str] = &["src", "Cargo.toml", "Cargo.lock"];

/// How far down the source tree the walk goes.
///
/// Not about Perch's, which is two deep. `list_dir` follows a link, so a
/// directory linked at one of its own ancestors would walk for ever — and a
/// wizard that hangs is a worse answer than any this guard can give.
const AS_DEEP_AS: usize = 16;

/// Refuses a `perch` that the source tree has moved past.
///
/// The footgun next door to the one above, and the reason the existence check
/// alone was not enough: `cargo run --bin dogfood-setup` builds the wizard and
/// relinks nothing else, so a `target/debug/perch` from last week satisfies
/// "there is a binary there" and the wizard drives it.
///
/// What makes that worth refusing rather than tolerating is that the *suite*
/// does not have the problem. `CARGO_BIN_EXE_perch` makes the binary a build
/// dependency of the test, so a run always drives a fresh one. Left alone, the
/// Export standing behind the marker is one Perch's work and the run that
/// marker admits is another's — and the Export is the half of it that has to be
/// trustworthy.
///
/// Everything it cannot see, it passes. This is here to catch a mistake rather
/// than to stand between somebody and their own machine, so a binary that will
/// not say when it was built, a source tree that is not there, and a file whose
/// age will not be read are all let through.
fn refuse_a_binary_the_source_has_moved_past(
    host: &dyn Host,
    perch: &Perch<'_>,
    sources: &Path,
) -> Result<()> {
    // A binary named on purpose is not one this source tree has anything to say
    // about. `$PERCH_DOGFOOD_BIN` is how somebody asks for an installed Perch —
    // the only way a bug in a release archive is ever caught — and an installed
    // Perch is older than the working copy nearly by definition.
    if host.env_var(BIN_VARIABLE).is_some() {
        return Ok(());
    }
    let Ok(built_at) = host.modified_at(perch.bin()) else {
        return Ok(());
    };
    let Some((newer, written_at)) = BUILT_FROM
        .iter()
        .filter_map(|name| newest_under(host, &sources.join(name), AS_DEEP_AS))
        .filter(|(_, when)| *when > built_at)
        .max_by_key(|(_, when)| *when)
    else {
        return Ok(());
    };

    Err(PerchError::Invalid(format!(
        "The Perch at {} was built at {}, and {} was written at {} — so it is \
         not the Perch this working copy describes.\n\
         `cargo run --bin dogfood-setup` builds the wizard and relinks nothing \
         else, so the Export about to be taken would be an older Perch's work, \
         while the suite this marker admits builds a fresh binary and drives \
         that instead. Run {HOW_TO_BUILD} first, or point {BIN_VARIABLE} at the \
         Perch you mean.",
        perch.bin().display(),
        stamp(built_at),
        newer.display(),
        stamp(written_at),
    )))
}

/// The newest file at or under `at`, by the time it was last written — or
/// nothing, where there is none or the walk could not see.
///
/// Written here rather than reached for from a crate because it is eight lines
/// and has to go through [`Host`]: a walk using `std::fs` directly is one no
/// test could arrange a stale tree for, which is the whole of what is being
/// tested.
fn newest_under(host: &dyn Host, at: &Path, depth: usize) -> Option<(PathBuf, DateTime<Utc>)> {
    if host.is_file(at) {
        return host
            .modified_at(at)
            .ok()
            .map(|when| (at.to_path_buf(), when));
    }
    if depth == 0 {
        return None;
    }
    host.list_dir(at)
        .ok()?
        .iter()
        .filter_map(|entry| newest_under(host, entry, depth - 1))
        .max_by_key(|(_, when)| *when)
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
        accounts: preflight.accounts.clone(),
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
        Some(path) => where_it_goes(host, path),
        None => {
            let default = default_export_path(host);
            let answered = crate::commands::ask(
                host,
                out,
                &format!("Where should the Export go? [{}] ", default.display()),
            )?;
            match answered.as_deref().map(str::trim) {
                Some("") | None => default,
                Some(typed) => where_it_goes(host, Path::new(typed)),
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

/// Where an Export goes when nobody says: beside the command, in the directory
/// it was typed in.
///
/// The home directory was the first answer and it was the wrong one — a file
/// somebody has to go and find later, in a directory they were not looking at.
/// The one place it must not be is under `~/.config/perch`, which is exactly
/// what `perch purge` deletes; anywhere else is a matter of who has to find it,
/// and that is whoever is sitting in the directory they ran the wizard from.
///
/// The repository is where that will nearly always be, so `*.age` is gitignored
/// and `tests/dogfood.rs` asserts the line is there — a working Credential for
/// every Account, one `git add -A` away from a public history, is not a trap to
/// leave lying under a default.
fn default_export_path(host: &dyn Host) -> PathBuf {
    let named = format!(
        "perch-dogfood-{}.age",
        host.now().format("%Y-%m-%dT%H-%M-%SZ")
    );
    match host.current_dir() {
        Ok(here) => here.join(named),
        Err(_) => PathBuf::from(named),
    }
}

/// A path somebody typed, as the filesystem will read it.
///
/// Two things the shell would have done and this prompt does not, because
/// nothing between the terminal and here is a shell:
///
/// A leading `~` is expanded. Typing one is the ordinary way to name a file in
/// the home directory, and taken literally it makes a directory *called* `~`
/// under wherever the wizard was run — which is not a place anybody would look
/// for the only copy of their Credentials.
///
/// What is left is resolved against the directory the command was typed in. A
/// relative path already reaches the right file, because `perch export` is a
/// child process and inherits the working directory — but the marker keeps this
/// path as its receipt, and `Export at dogfood.age` is a sentence that stops
/// being true the moment somebody reads it from anywhere else.
///
/// `~someone` is left alone. Resolving another user's home is a lookup Perch
/// has no business doing, and a literal path is at least one the error message
/// will name in full.
fn where_it_goes(host: &dyn Host, typed: &Path) -> PathBuf {
    let expanded = match typed.strip_prefix("~") {
        Ok(rest) => match host.home_dir() {
            Ok(home) => home.join(rest),
            Err(_) => typed.to_path_buf(),
        },
        Err(_) => typed.to_path_buf(),
    };

    if expanded.is_absolute() {
        return expanded;
    }
    match host.current_dir() {
        Ok(here) => here.join(expanded),
        Err(_) => expanded,
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
        covering(&[])
    }

    /// The marker a wizard run on a machine holding exactly these would have
    /// written: an Export, taken over all of them.
    fn covering(accounts: &[&str]) -> Marker {
        Marker {
            version: MARKER_VERSION,
            marked_at: "2026-08-12T09:00:00Z".parse().unwrap(),
            perch_version: "0.1.1".to_string(),
            export: Held::Exported {
                path: PathBuf::from("/Users/someone/perch-2026-08-12.age"),
                accounts: accounts.iter().map(|email| (*email).to_string()).collect(),
            },
            arrangement: Arrangement::default(),
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
                r#"{"version": 3, "marked_at": "2026-08-12T09:00:00Z",
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

    fn proves_nothing(_: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
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
                .any(|line| line == "This machine can prove up to 1 of 2 phases"),
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
                .any(|line| line == "This machine can prove up to 2 of 2 phases")
        );
    }

    // ---- the run ----------------------------------------------------------

    /// Marks the machine as it stands, which is the only marking a wizard can
    /// do: the Export it is a receipt for is taken over what is there when it
    /// runs. So anything a test wants covered has to be held *before* this.
    fn marked(host: FakeHost) -> FakeHost {
        let held = Preflight::taken(&host)
            .map(|preflight| preflight.accounts)
            .unwrap_or_default();
        let covered: Vec<&str> = held.iter().map(String::as_str).collect();
        mark(&host, &covering(&covered)).expect("the wizard marked it");
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
        fn touches_it(_: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
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

    /// Mark a fresh machine, `perch add` over the following weeks, run the
    /// suite. The marker said what was true when it was written and nothing
    /// revisited it, so the run went ahead and moved real Credentials around
    /// with no Export behind them — while the report said "no Export: this
    /// machine held no Accounts".
    #[test]
    fn a_machine_that_has_gained_an_account_since_it_was_marked_is_refused() {
        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())));
        now_holding(&host, &[("later@example.com".to_string(), None)]);

        let refused = a_run(&host, &[a_phase("would act", Needs::NOTHING)])
            .expect_err("nothing covers that login");

        assert!(
            refused.to_string().contains("later@example.com"),
            "{refused}"
        );
        assert!(refused.to_string().contains("dogfood-setup"), "{refused}");
        // The most important refusal the harness has, and it was rendering with
        // ten-space gaps mid-sentence and hard-wrapped nine-space indents: the
        // literal was written across lines with no `\` continuations, unlike
        // `HOW_TO_SET_UP` two lines below it. Every assertion on it was a
        // `contains`, which is exactly the shape that cannot see whitespace.
        let said = refused.to_string();
        assert!(
            !said.contains("  "),
            "no run of spaces inside a sentence: {said:?}"
        );
        assert!(
            said.lines().all(|line| !line.starts_with(' ')),
            "and no line indented as though it were a continuation: {said:?}"
        );
        assert!(
            relogins(&host).is_empty(),
            "and the Repair never got the terminal: {:?}",
            relogins(&host)
        );
    }

    /// The other side of it: an Export taken over the Accounts that are there
    /// is a receipt for those Accounts, and the run proceeds.
    #[test]
    fn a_machine_the_export_still_covers_runs() {
        let host = a_machine(&[("one@example.com", None)], &[]);

        a_run(&host, &[a_phase("runs anywhere", Needs::NOTHING)])
            .expect("the marker covers what this machine holds");
    }

    /// The reading `usable` was added for, closed against the phase two lines
    /// above rather than only against yesterday's run on another machine.
    #[test]
    fn a_phase_is_gated_on_the_machine_the_phase_before_it_left() {
        fn breaks_one(perch: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
            perch.interactive(&["relogin", "two@example.com"])?;
            Ok(vec!["it left an Account Quarantined".to_string()])
        }

        let host = with_a_claude_code(with_a_perch(a_bare_machine())).with_login(|host, _| {
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
        });
        now_holding(
            &host,
            &[
                ("one@example.com".to_string(), None),
                ("two@example.com".to_string(), None),
            ],
        );
        let host = marked(host);

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
        fn stops(_: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
            Err(Setback::perch("`perch switch` landed on the wrong Account")
                .leaving(&["two@example.com is active".to_string()])
                .put_back_with(&["perch switch one@example.com".to_string()])
                .into())
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
            written.contains("This machine can prove up to 1 of 1 phase"),
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
    ///
    /// With somebody at the terminal, because a Repair is a browser round trip
    /// and a run only walks those where attendance was opted into (ADR 0038).
    /// A machine holding a Quarantined Account and nobody watching is a machine
    /// whose Repair skips, which is what `a_bare_machine` is for.
    fn a_machine(held: &[(&str, Option<Quarantine>)], logins: &[(&str, Login)]) -> FakeHost {
        let host =
            with_a_claude_code(with_a_perch(a_bare_machine())).with_env(ATTENDED_VARIABLE, "1");

        let holds: Vec<(String, Option<Quarantine>)> = held
            .iter()
            .map(|(email, quarantine)| ((*email).to_string(), *quarantine))
            .collect();
        now_holding(&host, &holds);

        // Marked after, because that is the order a wizard runs in: it Exports
        // what the machine holds and writes the receipt for it.
        let host = marked(host);

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
            said.contains("This machine can prove up to 0 of 1 phase"),
            "what it could prove when it was found: {said}"
        );
        assert!(
            said.contains("This machine can now prove up to 1 of 1 phase"),
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
            said.contains("This machine can now prove up to 0 of 1 phase"),
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

    /// A Repair opens a browser and waits, so it is an attended act and asks
    /// the question every attended phase asks (ADR 0038). It runs ahead of the
    /// Phases and so ahead of their guard, which is how it came to be the one
    /// part of a run that assumed attendance: a cron job, or a run with its
    /// output piped, would have had `claude /login` opened on it and blocked
    /// there on an answer nobody was coming to give.
    #[test]
    fn a_repair_where_nobody_is_watching_is_skipped_rather_than_opening_a_browser() {
        let host = with_a_claude_code(with_a_perch(a_bare_machine()));
        now_holding(
            &host,
            &[(
                "one@example.com".to_string(),
                Some(Quarantine::RenewalRejected),
            )],
        );
        let host = marked(host);

        let (run, said) = a_watched_run(&host, &[]).expect("a run without a person still runs");

        assert!(
            relogins(&host).is_empty(),
            "no browser is opened where nothing can be typed at: {said}"
        );
        assert_eq!(
            run.repair.left_quarantined(),
            vec!["one@example.com"],
            "and the Account is reported as still Quarantined rather than as repaired"
        );
        assert!(
            said.contains("nobody is at the terminal"),
            "the skip says what is missing, and what is missing is a person: {said}"
        );
        assert!(
            said.contains(ATTENDED_VARIABLE),
            "and how to say they are there: {said}"
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
        let repair = Repair::NotRead {
            fault: Fault::Perch,
            why: "`perch list --json` exited 1".to_string(),
        };

        assert_eq!(
            repair.finish_by_hand(),
            vec!["perch list --json".to_string()]
        );
    }

    /// And it says whose fault it was rather than assuming Perch's.
    ///
    /// `read_as` classifies a held registry, a live Profile and an unavailable
    /// keychain as `Fault::Upstream` deliberately: a machine somebody works on
    /// runs other Perches, and a `perch watch` holding the lock for the second
    /// the Repair opened in is not a bug. The verdict dropped that and said
    /// "This is a fault in Perch" over every one of them — the misclassification
    /// ADR 0037 says gets a suite's red ignored within a month.
    #[test]
    fn a_listing_the_repair_could_not_read_says_whose_fault_it_was() {
        let upstream = Repair::NotRead {
            fault: Fault::Upstream,
            why: "another Perch holds the registry".to_string(),
        };
        let said = upstream.said().join("\n");
        assert!(said.contains(Fault::Upstream.said()), "{said}");
        assert!(
            !said.contains(Fault::Perch.said()),
            "a busy machine is not a bug to go and fix: {said}"
        );

        let ours = Repair::NotRead {
            fault: Fault::Perch,
            why: "`perch list --json` exited 1".to_string(),
        };
        assert!(
            ours.said().join("\n").contains(Fault::Perch.said()),
            "and one that really is Perch's still says so"
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
            written.contains("- This machine can prove up to 1 of 1 phase"),
            "{written}"
        );
        assert!(
            written.contains("- This machine can now prove up to 1 of 1 phase"),
            "{written}"
        );
    }

    /// The Repair happens before any phase acts, so no phase is ever pointed at
    /// an Account that was never going to work.
    #[test]
    fn nothing_acts_until_the_repair_has_had_its_turn() {
        fn reads_the_machine(perch: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
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

        assert!(
            matches!(run.repair, Repair::NotRead { .. }),
            "{:?}",
            run.repair
        );
        assert!(said.contains("Nothing could be repaired"), "{said}");
        assert!(said.contains("This is a fault in Perch."), "{said}");
    }

    // ---- the wizard -------------------------------------------------------

    /// The source tree the wizard is told `/build/perch` came from. Empty on
    /// most fixtures, which is one of the two ways the staleness guard has
    /// nothing to say — the other being a `perch` with no age recorded.
    const SOURCES: &str = "/repo";

    fn set_it_up(host: &dyn Host, args: &SetupArgs) -> Result<(Marker, String)> {
        let mut said = Vec::new();
        let marker = set_up(
            host,
            "/build/perch",
            Path::new(SOURCES),
            args,
            &[],
            &mut said,
        )?;
        Ok((marker, String::from_utf8(said).unwrap()))
    }

    /// A bare machine with the binary the wizard is about to drive on it. Every
    /// wizard test needs one, because a wizard with nothing to drive is refused
    /// before it does anything else.
    ///
    /// No age, deliberately: [`FakeHost::with_file`] records none, so the
    /// staleness guard cannot see this binary and passes it. Every wizard test
    /// but the ones below is therefore about what it was about before.
    fn with_a_perch(host: FakeHost) -> FakeHost {
        host.with_file("/build/perch", "")
    }

    /// A machine where `/build/perch` was built at `built`, and one source file
    /// under [`SOURCES`] was written at `written`.
    fn built_then_written(built: DateTime<Utc>, at: &str, written: DateTime<Utc>) -> FakeHost {
        a_bare_machine()
            .with_file_written_at("/build/perch", built)
            .with_file_written_at(format!("{SOURCES}/{at}"), written)
    }

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 8, 14, hour, minute, 0).unwrap()
    }

    /// The directory a fixture machine's commands are typed in — `FakeHost`'s
    /// working directory, and where an Export now goes by default.
    const TYPED_IN: &str = "/Users/someone/work";

    /// A path spelled the way `Path::join` spells it *here*, for the assertions
    /// that read a rendered message rather than compare two paths.
    ///
    /// `/repo/src/switch.rs` is the right path on every platform and the wrong
    /// string on one: `join` answers with Windows' separator there, so a message
    /// built by joining says `\` and a literal looking for `/` finds nothing.
    /// Comparing two `PathBuf`s is safe — that is by components — and this is
    /// for the times a message is the thing under test.
    fn spelled(base: &str, tail: &str) -> String {
        tail.split('/')
            .fold(PathBuf::from(base), |path, name| path.join(name))
            .display()
            .to_string()
    }

    /// The footgun the existence check above does not catch: `cargo run --bin
    /// dogfood-setup` relinks the wizard and leaves `target/debug/perch` where
    /// it was, so a binary from last week is there to be driven. The suite has
    /// no such problem — `CARGO_BIN_EXE_perch` rebuilds it — so a stale wizard
    /// takes the Export with one Perch and admits a run of another.
    #[test]
    fn a_perch_older_than_the_source_beside_it_is_refused_and_the_newer_file_named() {
        let host = built_then_written(at(9, 0), "src/switch.rs", at(9, 30));

        let refused = set_it_up(
            &host,
            &SetupArgs {
                unattended: true,
                ..SetupArgs::default()
            },
        )
        .expect_err("that binary is not what this working copy describes");

        let said = refused.to_string();
        assert!(said.contains(&spelled(SOURCES, "src/switch.rs")), "{said}");
        assert!(said.contains("cargo build --features dogfood"), "{said}");
        assert!(marker(&host).is_err(), "a refused setup marks nothing");
    }

    /// The walk goes down rather than across the top: nearly every file that
    /// changes Perch's behaviour is in a subdirectory of `src`, so a guard
    /// reading only the top level would pass on almost every stale tree there is.
    #[test]
    fn a_newer_file_is_found_however_deep_in_the_tree_it_sits() {
        let host = built_then_written(at(9, 0), "src/commands/switch.rs", at(9, 30));

        let refused = set_it_up(&host, &SetupArgs::default())
            .expect_err("a nested source file is still source");

        assert!(
            refused
                .to_string()
                .contains(&spelled(SOURCES, "src/commands/switch.rs")),
            "{refused}"
        );
    }

    /// A dependency bump changes the binary as surely as an edit does, and it is
    /// the case somebody is least likely to connect to a `perch` behaving oddly.
    #[test]
    fn a_newer_cargo_lock_is_a_stale_binary_too() {
        let host = built_then_written(at(9, 0), "Cargo.lock", at(9, 30));

        let refused = set_it_up(&host, &SetupArgs::default())
            .expect_err("that binary was linked against something else");

        assert!(refused.to_string().contains("Cargo.lock"), "{refused}");
    }

    /// The ordinary case, and the one that must not become a refusal: a build
    /// somebody has just run is newer than everything it was built from.
    #[test]
    fn a_perch_newer_than_all_of_the_source_is_what_the_guard_is_looking_for() {
        let host = built_then_written(at(9, 30), "src/switch.rs", at(9, 0))
            .with_file_written_at(format!("{SOURCES}/Cargo.toml"), at(8, 0));

        let (wrote, _) = set_it_up(
            &host,
            &SetupArgs {
                unattended: true,
                ..SetupArgs::default()
            },
        )
        .expect("this binary is the one this working copy describes");

        assert_eq!(wrote.export, Held::NothingHeld);
    }

    /// `$PERCH_DOGFOOD_BIN` is how somebody says which Perch they mean — an
    /// installed one, to catch a bug in a release archive. It is older than the
    /// working copy nearly by definition, and refusing it would break the one
    /// case the variable exists for.
    #[test]
    fn a_perch_named_on_purpose_is_never_refused_for_being_older_than_the_source() {
        let host = built_then_written(at(9, 0), "src/switch.rs", at(9, 30))
            .with_file_written_at("/usr/local/bin/perch", at(1, 0))
            .with_env(BIN_VARIABLE, "/usr/local/bin/perch");

        let (wrote, _) = set_it_up(
            &host,
            &SetupArgs {
                unattended: true,
                ..SetupArgs::default()
            },
        )
        .expect("that Perch was asked for by name");

        assert_eq!(wrote.export, Held::NothingHeld);
    }

    /// A machine that gets as far as the Export: one Account to save, a `perch
    /// export` that works, and `answers` for the one question the wizard asks.
    fn a_machine_that_exports(answers: &[&str]) -> FakeHost {
        with_a_perch(holding(
            a_bare_machine(),
            &[an_account("one@example.com", None)],
        ))
        .with_answers(answers)
        .with_login(|_, _| crate::error::EXIT_OK)
    }

    /// Where the wizard's Export ended up, according to the receipt it wrote.
    fn exported_to(wrote: &Marker) -> PathBuf {
        match &wrote.export {
            Held::Exported { path, .. } => path.clone(),
            held => panic!("an Export was taken, not {held:?}"),
        }
    }

    /// The home directory was the first answer and it was the wrong one: a file
    /// somebody has to go and find later, somewhere they were not looking.
    #[test]
    fn the_default_export_goes_in_the_directory_the_command_was_typed_in() {
        let host = a_machine_that_exports(&[""]);

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("the Export worked");

        let path = exported_to(&wrote);
        assert!(
            path.starts_with(TYPED_IN),
            "the Export went to {} rather than the directory the wizard was run \
             from",
            path.display()
        );
        assert!(
            said.contains(&format!("[{}", spelled(TYPED_IN, "perch-dogfood-"))),
            "the default is offered as the path it will be: {said}"
        );
    }

    /// Nothing between the terminal and the wizard is a shell, so nothing has
    /// expanded this. Taken literally it makes a directory *called* `~` under
    /// wherever the wizard was run, which is not where anybody would look for
    /// the only copy of their Credentials.
    #[test]
    fn a_typed_tilde_reaches_the_home_directory_rather_than_a_directory_called_that() {
        let host = a_machine_that_exports(&["~/Downloads/dogfood-test.age"]);

        let (wrote, _) = set_it_up(&host, &SetupArgs::default()).expect("the Export worked");

        assert_eq!(
            exported_to(&wrote),
            PathBuf::from("/Users/someone/Downloads/dogfood-test.age")
        );
    }

    /// Resolving another user's home is a lookup Perch has no business doing,
    /// and a path left alone is at least one the failure will name in full.
    #[test]
    fn a_tilde_naming_somebody_else_is_left_as_it_was_typed() {
        let host = a_machine_that_exports(&["~someone/export.age"]);

        let (wrote, _) = set_it_up(&host, &SetupArgs::default()).expect("the Export worked");

        assert_eq!(
            exported_to(&wrote),
            PathBuf::from("/Users/someone/work/~someone/export.age"),
            "resolved against the working directory, and otherwise untouched"
        );
    }

    /// A relative path already reaches the right file — `perch export` is a
    /// child process and inherits the working directory. It is the *marker*
    /// that needs the long spelling: `Export at export.age` is a receipt that
    /// stops being true the moment somebody reads it from another directory.
    #[test]
    fn a_relative_path_is_written_down_as_the_one_it_reaches() {
        let host = a_machine_that_exports(&["export.age"]);

        let (wrote, _) = set_it_up(&host, &SetupArgs::default()).expect("the Export worked");

        assert_eq!(
            exported_to(&wrote),
            PathBuf::from(TYPED_IN).join("export.age")
        );
        assert!(
            wrote
                .export
                .said()
                .contains(&spelled(TYPED_IN, "export.age")),
            "the receipt is read later and from somewhere else: {}",
            wrote.export.said()
        );
    }

    /// The flag comes through a shell, which expands its own `~` — but not the
    /// relative path it may equally be given, and the receipt has the same
    /// problem either way. One rule for a path, wherever it arrived from.
    #[test]
    fn a_path_given_as_a_flag_is_resolved_the_same_way_a_typed_one_is() {
        let host = a_machine_that_exports(&[]);

        let (wrote, _) = set_it_up(
            &host,
            &SetupArgs {
                export_to: Some(PathBuf::from("~/export.age")),
                ..SetupArgs::default()
            },
        )
        .expect("the Export worked");

        assert_eq!(
            exported_to(&wrote),
            PathBuf::from("/Users/someone/export.age")
        );
    }

    /// A guard that cannot see is not a guard that refuses. Neither a binary
    /// whose age will not be read nor a source tree that is not there says
    /// anything about staleness, and both are ordinary: a `perch` copied into
    /// place, a repository built on another machine.
    #[test]
    fn a_binary_or_a_source_tree_the_guard_cannot_see_is_passed_rather_than_refused() {
        for host in [
            // An age for the source and none for the binary.
            a_bare_machine()
                .with_file("/build/perch", "")
                .with_file_written_at(format!("{SOURCES}/src/switch.rs"), at(9, 30)),
            // An age for the binary and no source tree at all.
            a_bare_machine().with_file_written_at("/build/perch", at(9, 0)),
        ] {
            let (wrote, _) = set_it_up(
                &host,
                &SetupArgs {
                    unattended: true,
                    ..SetupArgs::default()
                },
            )
            .expect("nothing here says the binary is stale");

            assert_eq!(wrote.export, Held::NothingHeld);
        }
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
                path: PathBuf::from("/tmp/perch.age"),
                accounts: vec!["one@example.com".to_string()],
            },
            "the marker is a receipt for a set, not only for a path"
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

    // ---- what the wizard was told, and who is watching ---------------------

    /// The arrangement is the wizard's word for two things nothing on a machine
    /// can check, so a marker carrying one is how a test states them.
    fn arranged(host: FakeHost, group: Option<&str>, logins: usize) -> FakeHost {
        let held = Preflight::taken(&host)
            .map(|preflight| preflight.accounts)
            .unwrap_or_default();
        let covered: Vec<&str> = held.iter().map(String::as_str).collect();
        let mut marker = covering(&covered);
        marker.arrangement = Arrangement {
            group: group.map(str::to_string),
            logins,
        };
        mark(&host, &marker).expect("the wizard marked it");
        host
    }

    /// An Account in a Group, which `an_account` never is: what a Cycle needs is
    /// a declaration somebody made, and the arrangement names which one.
    fn grouped(email: &str, group: &str) -> crate::registry::Account {
        crate::registry::Account {
            group: Some(group.to_string()),
            ..an_account(email, None)
        }
    }

    /// Reading what an older Perch wrote is exactly what this repository does
    /// not do — and the sentence has to send somebody to the wizard rather than
    /// to the file.
    #[test]
    fn a_marker_from_an_older_perch_is_refused_and_says_how_to_replace_it() {
        let host = a_bare_machine().with_file(
            "/tmp/perch/dogfood.json",
            r#"{"version": 1, "marked_at": "2026-08-12T09:00:00Z",
                "perch_version": "0.1.0", "export": {"kind": "nothing-held"}}"#,
        );

        let refused = marker(&host).expect_err("a marker written before the arrangement");

        assert!(matches!(refused, PerchError::Invalid(_)));
        assert!(refused.to_string().contains("dogfood-setup"), "{refused}");
        assert!(
            !refused.to_string().contains("newer Perch"),
            "an older marker and a newer one are two different problems: {refused}"
        );
    }

    /// The skip line names what is missing, and what is missing on a runner is a
    /// person. Every attended phase also needs Accounts, so a machine holding
    /// none would otherwise report the true thing that helps nobody.
    #[test]
    fn a_phase_that_hands_the_terminal_over_skips_where_nobody_is_watching() {
        let attended = Needs {
            attended: true,
            ..Needs::AN_ACCOUNT
        };
        let preflight = Preflight::taken(&a_bare_machine()).unwrap();

        let why = preflight.unmet(&attended).expect("nobody is here");

        assert!(why.contains("nobody is at the terminal"), "{why}");
        assert!(why.contains(ATTENDED_VARIABLE), "{why}");
    }

    /// A hang is worse than any skip, and it is what believing the variable on
    /// its own would buy: an opt-in exported into a CI job or carried in from a
    /// shell profile reaches machines where nothing can be typed at all.
    #[test]
    fn asking_for_attendance_where_nothing_could_be_asked_is_refused_before_anything_acts() {
        fn touches_it(_: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
            panic!("a run refused at the top must not reach a phase");
        }

        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())))
            .with_env(ATTENDED_VARIABLE, "1")
            .without_terminal();

        let refused = a_run(
            &host,
            &[Phase {
                name: "would act",
                needs: Needs::NOTHING,
                prove: touches_it,
            }],
        )
        .expect_err("nothing here could ask a question");

        assert!(
            refused.to_string().contains("where you can type"),
            "the refusal has to say what to do about it: {refused}"
        );
    }

    #[test]
    fn attendance_asked_for_at_a_terminal_that_can_answer_admits_the_phase() {
        let host = a_machine(&[("one@example.com", None)], &[]).with_env(ATTENDED_VARIABLE, "1");
        let attended = Needs {
            attended: true,
            ..Needs::AN_ACCOUNT
        };

        let (run, said) = a_watched_run(&host, &[a_phase("hands it over", attended)])
            .expect("a marked machine runs");

        assert!(matches!(run.outcomes[0].1, Outcome::Proved(_)), "{run:?}");
        assert!(said.contains("Attended: yes"), "{said}");
    }

    /// The arranged Group and not the largest one: a phase Cycling through a
    /// Group somebody declared for their own reasons is the surprise the marker
    /// exists to prevent, arriving after the marker check has passed.
    #[test]
    fn a_pair_is_counted_in_the_group_that_was_set_aside_and_nowhere_else() {
        let host = arranged(
            holding(
                a_bare_machine(),
                &[
                    grouped("one@example.com", "work"),
                    grouped("two@example.com", "work"),
                    grouped("three@example.com", "personal"),
                ],
            ),
            Some("personal"),
            3,
        );
        let mut preflight = Preflight::taken(&host).unwrap();
        preflight.arranged(&marker(&host).unwrap(), false);

        let why = preflight
            .unmet(&Needs::A_PAIR)
            .expect("`personal` holds one");

        assert!(why.contains("`personal`"), "{why}");
        assert!(
            !why.contains("work"),
            "the Group somebody declared for their own reasons is not this suite's: {why}"
        );
    }

    #[test]
    fn a_machine_with_no_group_set_aside_is_sent_to_the_wizard_rather_than_to_a_count() {
        let host = arranged(
            holding(
                a_bare_machine(),
                &[
                    an_account("one@example.com", None),
                    an_account("two@example.com", None),
                ],
            ),
            None,
            2,
        );
        let mut preflight = Preflight::taken(&host).unwrap();
        preflight.arranged(&marker(&host).unwrap(), false);

        let why = preflight
            .unmet(&Needs::A_PAIR)
            .expect("nothing is set aside");

        assert!(why.contains("no Group is set aside"), "{why}");
        assert!(why.contains("dogfood-setup"), "{why}");
    }

    /// Arithmetic rather than a question, and the whole of what makes walking a
    /// real browser login safe: it happens on the machine that is behind.
    #[test]
    fn only_a_machine_holding_fewer_accounts_than_there_are_logins_can_prove_an_add() {
        let spare = Needs {
            spare_login: true,
            ..Needs::NOTHING
        };
        let one_here = holding(a_bare_machine(), &[an_account("one@example.com", None)]);

        let mut behind = Preflight::taken(&arranged(one_here, None, 2)).unwrap();
        behind.arrangement = Arrangement {
            group: None,
            logins: 2,
        };
        assert_eq!(behind.unmet(&spare), None);

        let mut complete = behind.clone();
        complete.arrangement.logins = 1;
        let why = complete.unmet(&spare).expect("this machine is not behind");
        assert!(why.contains("no login here left to add"), "{why}");
    }

    // ---- running one phase rather than the hour of them --------------------

    #[test]
    fn a_run_narrowed_to_one_phase_is_a_run_about_one_phase() {
        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())))
            .with_env(PHASES_VARIABLE, "second");
        let phases = [
            a_phase("the first one", Needs::NOTHING),
            a_phase("the second one", Needs::NOTHING),
        ];

        let (run, said) = a_watched_run(&host, &phases).expect("a marked machine runs");

        assert_eq!(run.outcomes.len(), 1, "{run:?}");
        assert_eq!(run.outcomes[0].0, "the second one");
        // The figure is about what the run is about. "1 of 2" under a run that
        // was never going to take the other one is a sentence describing a suite
        // nobody asked for.
        assert!(
            said.contains("can prove up to 1 of 1 phase"),
            "the figure counts the phases this run is about: {said}"
        );
        let written = host
            .read_file(&Path::new("/tmp/reports").join(run.file_name()))
            .expect("the report was written");
        assert!(written.contains("Narrowed to `PERCH_DOGFOOD_PHASES=second`"));
    }

    /// The one way a filter fails quietly: a run of nothing reports "0 of 0
    /// phases", which is the document a suite with nothing left to prove would
    /// write. The answer to "did Windows pass?" would then be yes.
    #[test]
    fn a_narrowing_that_matches_no_phase_is_refused_rather_than_proving_nothing() {
        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())))
            .with_env(PHASES_VARIABLE, "recncile");
        let phases = [a_phase(
            "a Run reconciles the Profile it launches",
            Needs::NOTHING,
        )];

        let refused = a_run(&host, &phases).expect_err("nothing matches that");

        assert!(matches!(refused, PerchError::NotFound(_)));
        assert!(
            refused.to_string().contains("a Run reconciles"),
            "a refusal has to name what could have been asked for: {refused}"
        );
    }

    #[test]
    fn an_unfiltered_run_still_means_every_phase_and_says_so() {
        let host = marked(with_a_claude_code(with_a_perch(a_bare_machine())));
        let phases = [a_phase("the only one", Needs::NOTHING)];

        let run = a_run(&host, &phases).expect("a marked machine runs");

        assert_eq!(run.filtered, None);
        let written = host
            .read_file(&Path::new("/tmp/reports").join(run.file_name()))
            .unwrap();
        assert!(written.contains("Every phase, unfiltered"), "{written}");
    }

    // ---- what the wizard asks for --------------------------------------------

    /// A machine holding two Accounts, an Export it can take, and a `perch
    /// group` that answers — everything the arrangement step needs to reach the
    /// end of its own questions.
    fn a_machine_to_arrange(accounts: &[crate::registry::Account], answers: &[&str]) -> FakeHost {
        let host = with_a_perch(holding(a_bare_machine(), accounts))
            .with_answers(answers)
            .with_login(|_, _| crate::error::EXIT_OK);
        let worked = Execution {
            status: crate::error::EXIT_OK,
            stdout: String::new(),
            stderr: String::new(),
        };
        let host = host.with_exec("/build/perch", &["group", "add", "dogfood"], worked.clone());
        accounts.iter().fold(host, |host, account| {
            host.with_exec(
                "/build/perch",
                &["group", "move", account.email(), "dogfood"],
                worked.clone(),
            )
        })
    }

    /// Both halves of the arrangement are things only a person knows, and the
    /// marker is where they are written down so no run has to guess at them.
    #[test]
    fn the_wizard_writes_down_what_it_was_told_about_the_group_and_the_logins() {
        let host = a_machine_to_arrange(
            &[
                an_account("one@example.com", None),
                an_account("two@example.com", None),
            ],
            &["", "3", "y"],
        );

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("it is set up");

        assert_eq!(
            wrote.arrangement,
            Arrangement {
                group: Some("dogfood".to_string()),
                logins: 3,
            }
        );
        assert!(
            said.contains("one@example.com is now in `dogfood`"),
            "{said}"
        );
        assert!(said.contains("3 logins held in all"), "{said}");
    }

    /// Declining costs the two Cycling phases and nothing else — a wizard that
    /// moved somebody's Accounts anyway would be doing the thing the marker
    /// exists to make impossible.
    #[test]
    fn a_group_nobody_agreed_to_is_not_declared_and_nothing_is_moved() {
        let host = a_machine_to_arrange(
            &[
                an_account("one@example.com", None),
                an_account("two@example.com", None),
            ],
            &["", "", "n"],
        );

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("it is set up");

        assert_eq!(wrote.arrangement.group, None);
        assert_eq!(
            wrote.arrangement.logins, 2,
            "an empty answer is the count held, which turns the `add` phase off"
        );
        assert!(said.contains("The phases that Cycle will skip"), "{said}");
    }

    /// A Group already holding a pair is the obvious answer, and is offered as
    /// one rather than declared beside it.
    #[test]
    fn a_group_that_already_holds_a_pair_is_the_one_offered() {
        let host = a_machine_to_arrange(
            &[
                grouped("one@example.com", "work"),
                grouped("two@example.com", "work"),
            ],
            &["", "2", ""],
        );

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("it is set up");

        assert_eq!(wrote.arrangement.group, Some("work".to_string()));
        assert!(said.contains("Set the Group `work` aside"), "{said}");
    }

    /// A number below what is on the machine is not one `spare_login` could act
    /// on — it is arithmetic between the two — so it is taken as the count held
    /// and said out loud rather than stored to puzzle somebody later.
    #[test]
    fn a_login_count_below_what_this_machine_holds_is_taken_as_what_it_holds() {
        let host = a_machine_to_arrange(
            &[
                an_account("one@example.com", None),
                an_account("two@example.com", None),
            ],
            &["", "1", "n"],
        );

        let (wrote, said) = set_it_up(&host, &SetupArgs::default()).expect("it is set up");

        assert_eq!(wrote.arrangement.logins, 2);
        assert!(said.contains("is taken as 2"), "{said}");
    }

    /// CI runs the wizard on every push, and a runner has nobody to ask.
    #[test]
    fn an_unattended_setup_asks_nothing_and_arranges_nothing() {
        let host = with_a_perch(a_bare_machine());

        let (wrote, _) = set_it_up(
            &host,
            &SetupArgs {
                unattended: true,
                ..SetupArgs::default()
            },
        )
        .expect("a runner holds nothing");

        assert_eq!(wrote.arrangement, Arrangement::default());
    }

    #[test]
    fn a_run_that_stopped_is_the_one_worth_having_written_down() {
        fn stops(_: &Perch<'_>, _: &Preflight, _: &mut dyn Write) -> Proof {
            Err(Setback::upstream("Anthropic was slow").into())
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
