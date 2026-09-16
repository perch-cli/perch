//! `perch triage` — the machine's evidence, handed to a coding agent
//! (ADR a-triage-hands-over-evidence).
//!
//! Perch gathers and hands over: it neither investigates nor files, so what it
//! owns is the evidence and the redaction on it.
//!
//! Changes nothing about the machine it describes, for a Probe's reasons
//! (ADR a-trail-is-evidence). The selected provider is launched bare rather than through a
//! Run, because Reconcile, Carry and a Marker are all machinery a Triage may be
//! investigating (ADR a-run-is-one-shot).

use std::io::Write;
use std::path::Path;

use crate::commands::probe::{Gathered, finding};
use crate::error::{EXIT_OK, PerchError, Result};
use crate::host::Host;
use crate::providers::provider::{DiagnosticSession, Id};
use crate::registry::{self, Active};
use crate::{commands, holdings, report, say};

/// The playbook the agent follows, and the repository's own copy of it. One
/// string rather than two that a test compares, because a second copy is a
/// second thing to keep right.
const PLAYBOOK: &str = include_str!("../../.github/triage/PLAYBOOK.md");

/// How many Triages are kept. Enough to hold the run before a fix beside the one
/// after it. What a Triage leaves is evidence, so losing an older one costs
/// nothing that running the command again does not give back.
const KEPT: usize = 3;

/// Findings that mean the selected provider would come up at a login prompt rather than at
/// a triage: each is a Credential, or a CLI, that Perch could not read.
/// Quoted from the Probe rather than asked again, so what withholds the launch
/// is the same sentence the evidence carries.
const WITHHOLDS_THE_LAUNCH: [&str; 4] = [
    finding::PROVIDER_UNREADABLE,
    finding::ASSUMPTION_BROKE,
    finding::KEYCHAIN_UNAVAILABLE,
    finding::STORE_UNREADABLE,
];

#[derive(Debug, clap::Args)]
pub struct TriageArgs {
    /// The model to hand the selected provider, where its own default will not do.
    ///
    /// Passed through untouched, and nothing by default: a model named in a
    /// released binary goes out of date on somebody else's schedule.
    #[arg(long)]
    pub model: Option<String>,

    /// Write the pasteable evidence with the email addresses, names and paths
    /// as they are.
    ///
    /// The copy the agent investigates from always has them. This is the copy
    /// meant for an issue, so it is placeholders unless you ask otherwise.
    #[arg(long)]
    pub raw: bool,
}

/// The three files one Triage writes, by the name the playbook calls each.
const PROMPT: &str = "prompt.md";
const RAW: &str = "probe.raw.txt";
const REDACTED: &str = "probe.txt";

/// Gathers, writes, and hands the terminal to the selected provider — or says why it did
/// not, which is an answer rather than a refusal: the evidence is on disk either
/// way, and that is the half of a Triage Perch owns.
pub fn run(host: &dyn Host, args: TriageArgs, out: &mut dyn Write) -> Result<i32> {
    let gathered = commands::probe::gathered(host);
    let at = holdings::triage_run_dir(host, host.now())?;
    host.create_private_dir_all(&at)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", at.display())))?;

    write_down(host, &at.join(RAW), &gathered.raw)?;
    write_down(
        host,
        &at.join(REDACTED),
        match args.raw {
            true => &gathered.raw,
            false => &gathered.redacted,
        },
    )?;
    write_down(host, &at.join(PROMPT), &seed(&at))?;
    prune(host);

    let Some(withheld) = withholding(host, &gathered) else {
        return launch(host, gathered.preferred_provider, &args, &at, out);
    };

    say::line(out, &withheld)?;
    say::line(out, "")?;
    for line in written(&at) {
        say::line(out, &line)?;
    }
    Ok(EXIT_OK)
}

/// One file of the evidence, or a refusal naming it. The only thing a Triage
/// can fail at: everything before it is reading, and everything after is
/// somebody else's session.
fn write_down(host: &dyn Host, at: &Path, contents: &str) -> Result<()> {
    host.write_private_file(at, contents)
        .map_err(|err| PerchError::Other(format!("could not write {}: {err}", at.display())))
}

/// What the agent is handed: where the evidence is, and the playbook whole.
///
/// The playbook is inlined rather than pointed at, so an agent that reads one
/// file has everything. The evidence is pointed at, because it is the part that
/// must not be summarized on the way past.
fn seed(at: &Path) -> String {
    format!(
        "Somebody's Perch is misbehaving and they started this session with \
         `perch triage`.\n\n\
         What Perch can see of this machine is written beside this file:\n\n\
         - `{}`, with the real names and paths, which is what you investigate from.\n\
         - `{}`, which is the same thing redacted, and what goes into an issue.\n\n\
         Follow the playbook below. Start by asking what went wrong.\n\n\
         ---\n\n\
         {PLAYBOOK}",
        at.join(RAW).display(),
        at.join(REDACTED).display(),
    )
}

/// Why the selected provider was not launched, or `None` where it will be.
///
/// Two questions, because a broken Credential shows up two ways: as a Probe that
/// could not read one, and as the Registry's own record that the Account it
/// belongs to is Quarantined.
fn withholding(host: &dyn Host, gathered: &Gathered) -> Option<String> {
    let unusable = |said: &str| {
        format!(
            "{} will not come up as this machine stands, so Perch has not \
             launched it. The Probe found:\n  {said}",
            gathered.preferred_provider.adapter().name(),
        )
    };

    if let Some(found) = gathered.found.iter().find(|found| {
        found
            .provider
            .is_none_or(|provider| provider == gathered.preferred_provider)
            && WITHHOLDS_THE_LAUNCH.contains(&found.code)
    }) {
        return Some(unusable(&found.said));
    }

    // Diagnosis must remain available when the manifest cannot be loaded.
    let registry = registry::load(host).ok().flatten()?;
    let Active::Settled(email) = registry.active_for(gathered.preferred_provider) else {
        return None;
    };
    let quarantine = registry.account(email)?.quarantine.as_ref()?;
    Some(unusable(&format!(
        "{email} is the active Account and it is Quarantined: {}.",
        quarantine.because()
    )))
}

/// Where the three files are, for the path that launches nothing. The launch
/// path names the directory in a note instead: somebody who is about to be
/// handed a session wants one line, and somebody who is not wants the paths.
fn written(at: &Path) -> Vec<String> {
    vec![
        "What Perch can see of this machine is written down:".to_string(),
        format!("  {}", at.join(PROMPT).display()),
        format!("  {}", at.join(RAW).display()),
        format!("  {}", at.join(REDACTED).display()),
        String::new(),
        format!(
            "Paste {PROMPT} into any coding agent to run the triage by hand, or \
             open an issue at {}.",
            report::ISSUES
        ),
    ]
}

/// The native provider owns the launch arguments and environment.
///
/// One argument rather than the playbook itself: a `.cmd` shim on Windows runs
/// through `cmd.exe`, which will not carry a multi-kilobyte multiline word.
fn launch(
    host: &dyn Host,
    provider: Id,
    args: &TriageArgs,
    at: &Path,
    out: &mut dyn Write,
) -> Result<i32> {
    let prompt = format!(
        "Read the file \"{}\" and follow its instructions exactly. It is your \
         Perch triage playbook, and it starts by asking what went wrong.",
        at.join(PROMPT).display()
    );
    let installation = provider.adapter().configured(host)?.installation(host)?;
    let prepared = installation.diagnostic_session(&DiagnosticSession {
        model: args.model.as_deref(),
        prompt: &prompt,
    })?;
    host.note(&format!(
        "What Perch can see of this machine is at {}. Starting {}, \
         which will ask what went wrong.",
        at.display(),
        provider.adapter().name()
    ));
    out.flush().map_err(say::failed)?;
    prepared.execute(host)
}

/// Drops all but the newest [`KEPT`] runs, this one among them.
///
/// Housekeeping rather than the job: a directory that will not list or will not
/// be removed is not something to fail a Triage over, and the next one tries
/// again.
fn prune(host: &dyn Host) {
    let Ok(dir) = holdings::triage_dir(host) else {
        return;
    };
    let Ok(entries) = host.list_dir(&dir) else {
        return;
    };
    let mut runs: Vec<_> = entries
        .into_iter()
        .filter_map(|path| Some((holdings::triage_run_started_at(&path)?, path)))
        .collect();
    runs.sort_by_key(|(started_at, _)| std::cmp::Reverse(*started_at));
    for (_, path) in runs.into_iter().skip(KEPT) {
        let _ = host.remove_dir_all(&path);
    }
}
