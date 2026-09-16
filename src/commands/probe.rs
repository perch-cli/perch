//! `perch probe` — everything Perch can see of this machine, in one paste
//! (ADR a-trail-is-evidence).
//!
//! Gathers rather than acts: no network, no Registry brought forward, no Trail
//! line of its own. Every failure on the way is a finding rather than a refusal,
//! since a Probe that refuses stops where the machine is worst.
//!
//! Judges only what Perch already computes to decide a refusal, quoting the exit
//! code that refusal carries. A rule invented here is asserted by nothing.

use std::io::Write;
use std::path::PathBuf;

use crate::error::{EXIT_OK, Result};
use crate::host::Host;
pub use crate::providers::provider::Finding;
use crate::providers::provider::{DiagnosticReport, Id, catalog};
use crate::redact::Redaction;
use crate::registry::Registry;
use crate::{commands, holdings, registry, say, service, trail, upgrade};

#[derive(Debug, clap::Args)]
pub struct ProbeArgs {
    /// Print what a script reads, rather than what a person does.
    #[arg(long)]
    pub json: bool,

    /// Print the email addresses, names and paths as they are.
    ///
    /// What leaves this machine is placeholders unless you ask otherwise, since
    /// what a Probe is for is being pasted somewhere else. The Account numbers
    /// are stable, so a report stays readable without them.
    #[arg(long)]
    pub raw: bool,
}

/// Named findings: a script counts these, and a Triage decides on four of them, so a
/// rename here is a rename everywhere rather than a literal that stops matching.
pub mod finding {
    pub use crate::providers::provider::diagnostic_code::*;
    pub const REGISTRY_UNREADABLE: &str = "registry-unreadable";
    pub const ACCOUNT_QUARANTINED: &str = "account-quarantined";
    pub const TRAIL_NOT_KEPT: &str = "trail-not-kept";
    pub const WATCHER_MAY_ACT_NOWHERE: &str = "watcher-may-act-nowhere";
    pub const COMMAND_NEVER_FINISHED: &str = "command-never-finished";
    pub const TRAIL_EMPTY: &str = "trail-empty";
}

/// The counts a Listing would show, gathered once for the two renderers.
struct Tally {
    accounts: usize,
    groups: usize,
    quarantined: usize,
    disabled: usize,
}

impl Tally {
    fn of(registry: &Registry) -> Tally {
        Tally {
            accounts: registry.accounts.len(),
            groups: registry.groups.len(),
            quarantined: registry
                .accounts
                .iter()
                .filter(|held| held.quarantine.is_some())
                .count(),
            disabled: registry
                .accounts
                .iter()
                .filter(|held| held.disabled)
                .count(),
        }
    }
}

/// Which Account is active, said once for both renderers so a Landing cannot be
/// visible at a terminal and absent from `--json`.
fn active_said(registry: &Registry, provider: Id, hidden: &Redaction) -> String {
    match registry.active_for(provider) {
        registry::Active::Nobody => "nobody".to_string(),
        registry::Active::Settled(email) => hidden.text(email),
        // A Switch written down and not yet recorded, which is the state a
        // killed Switch leaves and the one worth seeing named.
        registry::Active::Landing { leaving, arriving } => format!(
            "a Landing from {} to {}",
            leaving
                .as_deref()
                .map_or("nobody".to_string(), |email| hidden.text(email)),
            hidden.text(arriving)
        ),
    }
}

fn defaults_said(
    registry: &Registry,
    hidden: &Redaction,
) -> std::collections::BTreeMap<Id, String> {
    catalog()
        .iter()
        .map(|provider| {
            let id = provider.id();
            (id, active_said(registry, id, hidden))
        })
        .collect()
}

/// Everything gathered, before anything is rendered or redacted.
struct Seen {
    channel: Option<String>,
    exe: Option<PathBuf>,
    providers: Vec<(Id, DiagnosticReport)>,
    home: Option<PathBuf>,
    registry: Option<Registry>,
    /// The manifest's stated version remains reportable when loading is refused.
    on_disk: Option<u64>,
    registry_said: Option<String>,
    watcher: Option<service::Standing>,
    trail: trail::Reading,
    findings: Vec<Finding>,
}

/// The placeholders this gathering redacts to. A Registry that would not load
/// numbers nothing, which is a Probe of a machine with no Registry rather than a
/// failure: the home directory is still hidden.
fn redaction_over(host: &dyn Host, seen: &Seen) -> Redaction {
    Redaction::of(
        seen.registry.as_ref().unwrap_or(&Registry::default()),
        host.home_dir().ok().as_deref(),
    )
}

/// One gathering, read both ways, and what it found.
///
/// All three at once because two gatherings a second apart could disagree about
/// the machine: an agent investigates from the raw reading and pastes the
/// redacted one (ADR a-triage-hands-over-evidence).
pub struct Gathered {
    pub preferred_provider: Id,
    pub raw: String,
    pub redacted: String,
    /// What it found, so a caller can act on one without parsing the rendering
    /// back apart.
    pub found: Vec<Finding>,
}

/// The Probe a Triage hands over, gathered once. Here rather than in the
/// command that wants it, because everything it reads is this module's.
pub fn gathered(host: &dyn Host) -> Gathered {
    let seen = gather(host);
    let hidden = redaction_over(host, &seen);
    Gathered {
        preferred_provider: seen
            .registry
            .as_ref()
            .map(|registry| registry.run_provider)
            .unwrap_or_default(),
        raw: lines(&seen, &Redaction::none()).join("\n"),
        redacted: lines(&seen, &hidden).join("\n"),
        found: seen.findings,
    }
}

/// Gathers once, redacts once, and renders the answer one of two ways.
///
/// Output that could not be written travels as it does from any other command:
/// a Probe's code says what it found, and a broken pipe is not a finding.
pub fn run(host: &dyn Host, args: ProbeArgs, out: &mut dyn Write) -> Result<i32> {
    let seen = gather(host);
    let hidden = match args.raw {
        true => Redaction::none(),
        false => redaction_over(host, &seen),
    };

    match args.json {
        true => say::json(out, &document(&seen, &hidden))?,
        false => {
            for line in lines(&seen, &hidden) {
                say::line(out, &line)?;
            }
        }
    }
    // Nought whatever it *found*: a code of its own would make
    // `perch probe | pbcopy` read as a command that failed, and every code Perch
    // has already names one refusal.
    Ok(EXIT_OK)
}

fn gather(host: &dyn Host) -> Seen {
    let mut findings = Vec::new();
    let trail_read = trail::read(host);

    let on_disk = holdings::registry_path(host)
        .ok()
        .and_then(|at| host.read_file(&at).ok())
        .and_then(|held| crate::error::claimed_version(&held));

    let (registry, registry_said) = match registry::load(host) {
        Ok(registry) => (registry, None),
        Err(err) => {
            findings.push(Finding::refused(finding::REGISTRY_UNREADABLE, &err));
            (None, Some(err.to_string()))
        }
    };

    if let Some(registry) = &registry {
        for account in registry
            .accounts
            .iter()
            .filter(|held| held.quarantine.is_some())
        {
            findings.push(Finding {
                provider: Some(account.provider()),
                code: finding::ACCOUNT_QUARANTINED,
                exit_code: Some(crate::error::EXIT_QUARANTINED),
                said: format!(
                    "{} is Quarantined, so Cycling will not choose it and a Switch \
                     to it refuses. `perch relogin {}` is the way back.",
                    account.identity.email, account.identity.email
                ),
            });
        }
        // Every command writes both, so a Registry newer than the Trail's last
        // line says the silent write failed. By a minute, the two times coming
        // from the filesystem's clock and from Perch's.
        let wrote = holdings::registry_path(host)
            .ok()
            .and_then(|at| host.modified_at(&at).ok());
        if let (Some(last), Some(wrote)) = (trail_read.last_written, wrote)
            && wrote - last > chrono::Duration::minutes(1)
        {
            findings.push(Finding::noticed(
                finding::TRAIL_NOT_KEPT,
                format!(
                    "The Registry was written at {} and the Trail's last line is \
                     from {}, so a command ran and wrote nothing down. Perch's \
                     home is most likely not writable.",
                    wrote.format("%Y-%m-%d %H:%M:%SZ"),
                    last.format("%Y-%m-%d %H:%M:%SZ")
                ),
            ));
        }
    }

    let preferred = registry
        .as_ref()
        .map(|registry| registry.run_provider)
        .unwrap_or_default();
    let mut providers = Vec::new();
    for provider in catalog() {
        let id = provider.id();
        let held = registry.as_ref().is_some_and(|registry| {
            registry
                .accounts
                .iter()
                .any(|account| account.provider() == id)
        });
        let mut report = provider.diagnose(host);
        if id != preferred && !held && report.path.is_none() {
            continue;
        }
        findings.append(&mut report.findings);
        providers.push((id, report));
    }

    let watcher = commands::service::asked_of_the_machine(host).ok();
    if let Some(standing) = &watcher
        && standing.installed
        && standing.any_scope_may_act == Some(false)
    {
        findings.push(Finding::noticed(
            finding::WATCHER_MAY_ACT_NOWHERE,
            "A Service is installed and no Scope has told the Watcher it may act, \
             so it holds every round rather than Switching anything. \
             `perch config set <scope> watcher-may-act true`."
                .to_string(),
        ));
    }

    for started in &trail_read.unfinished {
        findings.push(Finding::noticed(
            finding::COMMAND_NEVER_FINISHED,
            format!(
                "`perch {}` was started at {} and the process that ran it is gone, \
                 so it never finished.",
                started.words.join(" "),
                started.at.format("%Y-%m-%d %H:%M:%SZ")
            ),
        ));
    }
    if trail_read.held == 0 {
        findings.push(Finding::noticed(
            finding::TRAIL_EMPTY,
            "The Trail holds nothing. Either no command has run here since this \
             Perch was installed, or Perch's home cannot be written to."
                .to_string(),
        ));
    }

    Seen {
        channel: upgrade::channel(host)
            .ok()
            .flatten()
            .map(|channel| format!("{channel:?}").to_lowercase()),
        exe: host.current_exe().ok(),
        providers,
        home: holdings::perch_home(host).ok(),
        registry,
        on_disk,
        registry_said,
        watcher,
        trail: trail_read,
        findings,
    }
}

/// What a person reads, in the order they read it: the judgment first, and the
/// facts under it standing on their own where the judgment is wrong.
fn lines(seen: &Seen, hidden: &Redaction) -> Vec<String> {
    let mut said = Vec::new();
    let the_column = crate::column::Labeled::of(0, 14);
    let column =
        move |name: &str, value: String| the_column.row(name, &crate::host::Shown::of(&value));

    said.push(match seen.findings.is_empty() {
        true => column("Findings", "nothing Perch would refuse over".to_string()),
        false => "Findings".to_string(),
    });
    for finding in &seen.findings {
        said.push(format!(
            "  {}{}",
            hidden.text(&finding.said),
            match finding.exit_code {
                Some(code) => format!(" (exit {code})"),
                None => String::new(),
            }
        ));
    }
    said.push(String::new());

    said.push(column(
        "Perch",
        format!(
            "{} ({} {}){}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            match &seen.channel {
                Some(channel) => format!(", installed by {channel}"),
                None => String::new(),
            }
        ),
    ));
    if let Some(exe) = &seen.exe {
        said.push(column("Binary", hidden.path(exe)));
    }
    // The version and the path are asked for separately, so a binary that is
    // there and will not answer `--version` names both.
    for (id, report) in &seen.providers {
        let version = match &report.version {
            Ok(version) => hidden.text(version),
            Err(said) => hidden.text(said),
        };
        said.push(column(
            id.adapter().name(),
            match &report.path {
                Some(at) => format!("{version}, at {}", hidden.path(at)),
                None => version,
            },
        ));
    }
    // Read off the version rather than off the loaded registry: a file that
    // states none is a file that would not load, so the two answers are one.
    said.push(column(
        "Home",
        match (&seen.home, &seen.registry_said) {
            (Some(home), Some(_)) => {
                format!("{}, and the Registry would not load", hidden.path(home))
            }
            (Some(home), None) => format!(
                "{}, {}",
                hidden.path(home),
                match seen.on_disk {
                    Some(stated) => format!("Registry version {stated}"),
                    None => "no Registry yet".to_string(),
                }
            ),
            (None, _) => "could not be found".to_string(),
        },
    ));
    if let Some(registry) = &seen.registry {
        said.push(column(
            "Active",
            defaults_said(registry, hidden)
                .into_iter()
                .map(|(id, state)| format!("{}: {state}", id.word()))
                .collect::<Vec<_>>()
                .join("; "),
        ));
        let tally = Tally::of(registry);
        said.push(column(
            "Holdings",
            format!(
                "{} in {}, {} Quarantined, {} Disabled",
                say::accounts(tally.accounts),
                say::groups(tally.groups),
                tally.quarantined,
                tally.disabled,
            ),
        ));
    }
    said.push(column(
        "Watcher",
        match &seen.watcher {
            None => "could not be asked about".to_string(),
            Some(standing) if !standing.installed => "no Service installed".to_string(),
            Some(standing) => format!(
                "installed, {}, {}{}",
                match standing.running {
                    true => "running",
                    false => "not running",
                },
                match standing.any_scope_may_act {
                    Some(true) => "may act somewhere",
                    Some(false) => "may act nowhere",
                    None => "and what it may act on is unknown",
                },
                match standing.binary_is_there {
                    Some(false) => ", and the binary its unit names is gone",
                    _ => "",
                }
            ),
        },
    ));
    // Named rather than read: reaching the journal means a subprocess three ways,
    // and a Watcher's decisions are in the Trail on every platform already
    // (ADR a-crate-must-not-cost-a-seam).
    if let Some(standing) = &seen.watcher
        && standing.installed
    {
        said.push(column(
            "Its log",
            hidden.text(&standing.manager.log_is_at(standing.log.as_deref())),
        ));
    }
    said.push(column(
        "Trail",
        match seen.trail.last_written {
            Some(at) => format!(
                "{} lines, last written {}",
                seen.trail.held,
                at.format("%Y-%m-%d %H:%M:%SZ")
            ),
            None => "nothing written".to_string(),
        },
    ));

    said.push(String::new());
    said.push("Assumptions".to_string());
    let verdicts = crate::column::Labeled::of(2, 9);
    for (id, report) in &seen.providers {
        for assumption in &report.assumptions {
            said.push(verdicts.row(
                assumption.status.said(),
                &crate::host::Shown::of(&format!(
                    "{}: {}",
                    id.word(),
                    hidden.text(&assumption.name)
                )),
            ));
        }
    }

    said.push(String::new());
    said.push("Trail".to_string());
    // One row an invocation rather than one a line: the two lines are how the
    // file survives a command that never comes back, and a reader wants what
    // was run beside what it exited with.
    let ended: std::collections::BTreeMap<&str, i32> = seen
        .trail
        .lines
        .iter()
        .filter_map(|line| Some((line.id.as_str(), line.exit_code?)))
        .collect();
    for line in seen
        .trail
        .lines
        .iter()
        .filter(|line| line.event != trail::Event::End)
    {
        said.push(format!(
            "  {}  {}{}{}",
            line.at.format("%H:%M:%S"),
            line.words
                .iter()
                .map(|word| hidden.word(word))
                .collect::<Vec<_>>()
                .join(" "),
            match line.passed_on {
                0 => String::new(),
                passed => format!(" -- {} to the client", say::words(passed)),
            },
            match (line.event, ended.get(line.id.as_str())) {
                // A Watcher round has nothing to end and no code to carry.
                (trail::Event::Acted, _) => String::new(),
                (_, Some(code)) => format!("  exit {code}"),
                (_, None) => "  no end line".to_string(),
            }
        ));
    }

    said
}

/// The same answers as keys, with `null` where the machine gave none — which is
/// what tells "the Registry would not load" from "no Accounts".
fn document(seen: &Seen, hidden: &Redaction) -> serde_json::Value {
    serde_json::json!({
        "perch": {
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "channel": seen.channel,
            "binary": seen.exe.as_ref().map(|at| hidden.path(at)),
        },
        "providers": seen.providers.iter().map(|(id, report)| serde_json::json!({
            "id": id.word(),
            "version": report.version.as_ref().ok().map(|version| hidden.text(version)),
            "path": report.path.as_ref().map(|at| hidden.path(at)),
            "said": report.version.as_ref().err().map(|said| hidden.text(said)),
            "assumptions": report.assumptions.iter().map(|assumption| serde_json::json!({
                "assumption": hidden.text(&assumption.name),
                "verdict": assumption.status.said(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "home": {
            "path": seen.home.as_ref().map(|at| hidden.path(at)),
            "registry_version": seen.on_disk,
            "registry_said": seen.registry_said.as_ref().map(|said| hidden.text(said)),
        },
        "holdings": seen.registry.as_ref().map(|registry| {
            let tally = Tally::of(registry);
            serde_json::json!({
                "active": defaults_said(registry, hidden),
                "accounts": tally.accounts,
                "groups": tally.groups,
                "quarantined": tally.quarantined,
                "disabled": tally.disabled,
            })
        }),
        "watcher": seen.watcher.as_ref().map(service::Standing::document),
        "findings": seen.findings.iter().map(|finding| serde_json::json!({
            "provider": finding.provider,
            "code": finding.code,
            "exit_code": finding.exit_code,
            "said": hidden.text(&finding.said),
        })).collect::<Vec<_>>(),
        "trail": {
            "held": seen.trail.held,
            "last_written": seen.trail.last_written,
            "lines": seen.trail.lines.iter().map(|line| serde_json::json!({
                "at": line.at,
                "event": match line.event {
                trail::Event::Start => "start",
                trail::Event::End => "end",
                trail::Event::Acted => "acted",
            },
            "words": line.words.iter().map(|word| hidden.word(word)).collect::<Vec<_>>(),
                "passed_on": line.passed_on,
                "exit_code": line.exit_code,
            })).collect::<Vec<_>>(),
        },
    })
}
