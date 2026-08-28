//! `perch probe` — everything Perch can see of this machine, in one paste
//! (ADR a-trail-is-evidence).
//!
//! Gathers rather than acts: no network, no registry brought forward, no Trail
//! line of its own. Every failure on the way is a finding rather than a refusal,
//! since a Probe that refuses stops where the machine is worst.
//!
//! Judges only what Perch already computes to decide a refusal, quoting the exit
//! code that refusal carries. A rule invented here is asserted by nothing.

use std::io::Write;
use std::path::PathBuf;

use crate::error::{EXIT_OK, PerchError, Result};
use crate::host::Host;
use crate::redact::Redaction;
use crate::registry::Registry;
use crate::{commands, holdings, probe, registry, say, service, trail, upgrade};

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

/// Whether an assumption held.
///
/// `Unread` is not a doubt about the assumption: it is the probe having stopped
/// before it got there, which is a different thing from a belief that failed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stood {
    Held,
    Broke,
    Unread,
}

impl Stood {
    fn said(self) -> &'static str {
        match self {
            Stood::Held => "held",
            Stood::Broke => "broke",
            Stood::Unread => "unread",
        }
    }
}

/// The assumptions in the order the probe reaches them, so everything after the
/// one that broke is honestly reported as never having been asked.
const REACHED: [&str; 6] = [
    probe::assumption::INSTALLED,
    probe::assumption::CREDENTIAL_LOCATION,
    probe::assumption::ACCOUNT_NAME,
    probe::assumption::CREDENTIAL_SHAPE,
    probe::assumption::IDENTITY_BLOCK,
    probe::assumption::SESSION_MARKER,
];

/// Something that would make a command refuse, with the code it would refuse
/// with. `code` is what a script counts; `said` is the same thing for a person.
struct Finding {
    code: &'static str,
    exit_code: Option<i32>,
    said: String,
}

impl Finding {
    /// A failure Perch already knows how to refuse over, said as it says it.
    fn refused(code: &'static str, err: &PerchError) -> Finding {
        Finding {
            code,
            exit_code: Some(err.exit_code()),
            said: err.to_string(),
        }
    }

    /// Something true of the machine that no single failure carries a code for.
    fn noticed(code: &'static str, said: String) -> Finding {
        Finding {
            code,
            exit_code: None,
            said,
        }
    }
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
fn active_said(registry: &Registry, hidden: &Redaction) -> String {
    match registry.active() {
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

/// Everything gathered, before anything is rendered or redacted.
struct Seen {
    channel: Option<String>,
    exe: Option<PathBuf>,
    claude: std::result::Result<String, String>,
    claude_at: Option<PathBuf>,
    home: Option<PathBuf>,
    registry: Option<Registry>,
    /// The version the file on disk states. Not the loaded registry's, which
    /// `registry::load` has already carried forward in memory — reporting that
    /// would say every machine is current and undo the point of the exemption.
    on_disk: Option<u64>,
    registry_said: Option<String>,
    watcher: Option<service::Standing>,
    assumptions: [Stood; REACHED.len()],
    trail: trail::Reading,
    findings: Vec<Finding>,
}

/// Gathers once, redacts once, and renders the answer one of two ways.
///
/// Output that could not be written travels as it does from any other command:
/// a Probe's code says what it found, and a broken pipe is not a finding.
pub fn run(host: &dyn Host, args: ProbeArgs, out: &mut dyn Write) -> Result<i32> {
    let seen = gather(host);
    let hidden = match args.raw {
        true => Redaction::none(),
        false => Redaction::of(
            seen.registry.as_ref().unwrap_or(&Registry::default()),
            host.home_dir().ok().as_deref(),
        ),
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

    let installed = probe::Installed::probed(host);
    let claude = match &installed {
        Ok(installed) => Ok(installed.version().to_string()),
        Err(err) => {
            findings.push(Finding::refused("claude-code-unreadable", err));
            Err(err.to_string())
        }
    };

    let on_disk = holdings::registry_path(host)
        .ok()
        .and_then(|at| host.read_file(&at).ok())
        .and_then(|held| crate::error::claimed_version(&held));

    let (registry, registry_said) = match registry::load(host) {
        Ok(registry) => (registry, None),
        Err(err) => {
            findings.push(Finding::refused("registry-unreadable", &err));
            (None, Some(err.to_string()))
        }
    };

    if let Some(registry) = &registry {
        if on_disk.is_some_and(|stated| stated < u64::from(registry::CURRENT_VERSION)) {
            findings.push(Finding::noticed(
                "registry-behind",
                format!(
                    "The registry on disk is version {} and this Perch writes \
                     version {}. No command has brought it forward yet, which the \
                     next one that reads it will do.",
                    on_disk.unwrap_or_default(),
                    registry::CURRENT_VERSION
                ),
            ));
        }
        for account in registry
            .accounts
            .iter()
            .filter(|held| held.quarantine.is_some())
        {
            findings.push(Finding {
                code: "account-quarantined",
                exit_code: Some(crate::error::EXIT_QUARANTINED),
                said: format!(
                    "{} is Quarantined, so Cycling will not choose it and a Switch \
                     to it refuses. `perch relogin {}` is the way back.",
                    account.identity.email, account.identity.email
                ),
            });
        }
        // Every command writes both, so a registry newer than the Trail's last
        // line says the silent write failed. By a minute, the two times coming
        // from the filesystem's clock and from Perch's.
        let wrote = holdings::registry_path(host)
            .ok()
            .and_then(|at| host.modified_at(&at).ok());
        if let (Some(last), Some(wrote)) = (trail_read.last_written, wrote)
            && wrote - last > chrono::Duration::minutes(1)
        {
            findings.push(Finding::noticed(
                "trail-not-kept",
                format!(
                    "The registry was written at {} and the Trail's last line is \
                     from {}, so a command ran and wrote nothing down. Perch's \
                     home is most likely not writable.",
                    wrote.format("%Y-%m-%d %H:%M:%SZ"),
                    last.format("%Y-%m-%d %H:%M:%SZ")
                ),
            ));
        }
    }

    let assumptions = asked(host, &mut findings);

    let watcher = commands::service::asked_of_the_machine(host).ok();
    if let Some(standing) = &watcher
        && standing.installed
        && standing.any_scope_may_act == Some(false)
    {
        findings.push(Finding::noticed(
            "watcher-may-act-nowhere",
            "A Service is installed and no Scope has told the Watcher it may act, \
             so it holds every round rather than Switching anything. \
             `perch config set <scope> watcher-may-act true`."
                .to_string(),
        ));
    }

    for started in &trail_read.unfinished {
        findings.push(Finding::noticed(
            "command-never-finished",
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
            "trail-empty",
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
        claude,
        claude_at: probe::claude_bin(host).ok(),
        home: holdings::perch_home(host).ok(),
        registry,
        on_disk,
        registry_said,
        watcher,
        assumptions,
        trail: trail_read,
        findings,
    }
}

/// Runs the one probe there is against the Default Profile's store, and reads
/// its outcome back onto the list of assumptions it passes through.
fn asked(host: &dyn Host, findings: &mut Vec<Finding>) -> [Stood; REACHED.len()] {
    let mut held = [Stood::Unread; REACHED.len()];
    let store = match probe::default_profile_store(host) {
        Ok(store) => store,
        Err(_) => return held,
    };

    match probe::probe(host, store) {
        // A Probe claims no Profile, so the last one is never reached.
        Ok(_) => {
            held = [Stood::Held; REACHED.len()];
            held[REACHED.len() - 1] = Stood::Unread;
        }
        Err(PerchError::ProbeRefused(refusal)) => {
            let broke = REACHED
                .iter()
                .position(|named| *named == refusal.assumption)
                .unwrap_or(0);
            for (at, one) in held.iter_mut().enumerate() {
                *one = match at.cmp(&broke) {
                    std::cmp::Ordering::Less => Stood::Held,
                    std::cmp::Ordering::Equal => Stood::Broke,
                    std::cmp::Ordering::Greater => Stood::Unread,
                };
            }
            findings.push(Finding {
                code: "assumption-broke",
                exit_code: Some(crate::error::EXIT_PROBE_REFUSED),
                said: refusal.to_string(),
            });
        }
        Err(err) => {
            held[0] = Stood::Held;
            let code = match err {
                PerchError::KeychainUnavailable(_) => "keychain-unavailable",
                _ => "store-unreadable",
            };
            findings.push(Finding::refused(code, &err));
        }
    }
    held
}

/// What a person reads, in the order they read it: the judgment first, and the
/// facts under it standing on their own where the judgment is wrong.
fn lines(seen: &Seen, hidden: &Redaction) -> Vec<String> {
    let mut said = Vec::new();
    let column = |name: &str, value: String| format!("{name:<14}{value}");

    said.push(match seen.findings.is_empty() {
        true => "Findings      nothing Perch would refuse over".to_string(),
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
    said.push(column(
        "Claude Code",
        match (&seen.claude, &seen.claude_at) {
            (Ok(version), Some(at)) => format!("{version}, at {}", hidden.path(at)),
            (Ok(version), None) => version.clone(),
            (Err(said), _) => hidden.text(said),
        },
    ));
    said.push(column(
        "Home",
        match (&seen.home, &seen.registry, &seen.registry_said) {
            (Some(home), Some(_), _) => format!(
                "{}, registry version {}",
                hidden.path(home),
                match seen.on_disk {
                    Some(stated) => stated.to_string(),
                    None => "unstated".to_string(),
                }
            ),
            (Some(home), None, Some(_)) => {
                format!("{}, and the registry would not load", hidden.path(home))
            }
            (Some(home), None, None) => format!("{}, no registry yet", hidden.path(home)),
            (None, _, _) => "could not be found".to_string(),
        },
    ));
    if let Some(registry) = &seen.registry {
        said.push(column("Active", active_said(registry, hidden)));
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
    for (assumption, held) in REACHED.iter().zip(seen.assumptions) {
        said.push(format!("  {:<9}{assumption}", held.said()));
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
/// what tells "the registry would not load" from "no Accounts".
fn document(seen: &Seen, hidden: &Redaction) -> serde_json::Value {
    serde_json::json!({
        "perch": {
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "channel": seen.channel,
            "binary": seen.exe.as_ref().map(|at| hidden.path(at)),
        },
        "claude_code": {
            "version": seen.claude.as_ref().ok(),
            "path": seen.claude_at.as_ref().map(|at| hidden.path(at)),
            "said": seen.claude.as_ref().err().map(|said| hidden.text(said)),
        },
        "home": {
            "path": seen.home.as_ref().map(|at| hidden.path(at)),
            "registry_version": seen.on_disk,
            "registry_said": seen.registry_said.as_ref().map(|said| hidden.text(said)),
        },
        "holdings": seen.registry.as_ref().map(|registry| {
            let tally = Tally::of(registry);
            serde_json::json!({
                "active": active_said(registry, hidden),
                "accounts": tally.accounts,
                "groups": tally.groups,
                "quarantined": tally.quarantined,
                "disabled": tally.disabled,
            })
        }),
        "watcher": seen.watcher.as_ref().map(service::Standing::document),
        "assumptions": REACHED.iter().zip(seen.assumptions).map(|(assumption, held)| {
            serde_json::json!({ "assumption": assumption, "verdict": held.said() })
        }).collect::<Vec<_>>(),
        "findings": seen.findings.iter().map(|finding| serde_json::json!({
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
