//! `perch triage` — the machine's evidence, handed to a coding agent
//! (ADR a-triage-hands-over-evidence).
//!
//! Perch gathers and hands over: it neither investigates nor files, so what it
//! owns is the evidence and the redaction on it.
//!
//! Changes nothing about the machine it describes, for a Probe's reasons
//! (ADR a-trail-is-evidence). Claude Code is launched bare rather than through a
//! Run, because Reconcile, Carry and a Marker are all machinery a Triage may be
//! investigating (ADR a-run-is-one-shot).

use std::io::Write;
use std::path::Path;

use crate::commands::probe::Gathered;
use crate::error::{EXIT_OK, PerchError, Result};
use crate::host::Host;
use crate::registry::{self, Active};
use crate::{commands, holdings, probe, say};

/// The playbook the agent follows, and the repository's own copy of it. One
/// string rather than two that a test compares, because a second copy is a
/// second thing to keep right.
const PLAYBOOK: &str = include_str!("../../.github/triage/PLAYBOOK.md");

/// Where a report ends up, for the two paths that print it.
const ISSUES: &str = "https://github.com/perch-cli/perch/issues";

/// How many Triages are kept. Enough to hold the run before a fix beside the one
/// after it. What a Triage leaves is evidence, so losing an older one costs
/// nothing that running the command again does not give back.
const KEPT: usize = 3;

/// Findings that mean Claude Code would come up at a login prompt rather than at
/// a triage: each is a Credential, or a Claude Code, that Perch could not read.
/// Quoted from the Probe rather than asked again, so what withholds the launch
/// is the same sentence the evidence carries.
const WITHHOLDS_THE_LAUNCH: [&str; 4] = [
    "claude-code-unreadable",
    "assumption-broke",
    "keychain-unavailable",
    "store-unreadable",
];

#[derive(Debug, clap::Args)]
pub struct TriageArgs {
    /// The model to hand Claude Code, where its own default will not do.
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

/// Gathers, writes, and hands the terminal to Claude Code — or says why it did
/// not, which is an answer rather than a refusal: the evidence is on disk either
/// way, and that is the half of a Triage Perch owns.
pub fn run(host: &dyn Host, args: TriageArgs, out: &mut dyn Write) -> Result<i32> {
    let gathered = commands::probe::gathered(host);
    let at = holdings::triage_run_dir(host, host.now())?;
    host.create_private_dir_all(&at)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", at.display())))?;

    wrote(host, &at.join(RAW), &gathered.raw)?;
    wrote(
        host,
        &at.join(REDACTED),
        match args.raw {
            true => &gathered.raw,
            false => &gathered.redacted,
        },
    )?;
    wrote(host, &at.join(PROMPT), &seed(&at))?;
    prune(host);

    let Some(withheld) = withholding(host, &gathered) else {
        return launch(host, &args, &at, out);
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
fn wrote(host: &dyn Host, at: &Path, contents: &str) -> Result<()> {
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

/// Why Claude Code was not launched, or `None` where it will be.
///
/// Two questions, because a broken Credential shows up two ways: as a Probe that
/// could not read one, and as the registry's own record that the Account it
/// belongs to is Quarantined.
fn withholding(host: &dyn Host, gathered: &Gathered) -> Option<String> {
    let unusable = |said: &str| {
        format!(
            "Claude Code will not come up as this machine stands, so Perch has not \
             launched it. The Probe found:\n  {said}"
        )
    };

    if let Some((_, said)) = gathered
        .found
        .iter()
        .find(|(code, _)| WITHHOLDS_THE_LAUNCH.contains(code))
    {
        return Some(unusable(said));
    }

    // Read and let go. A Triage brings no registry forward and saves none: the
    // migration is one of the things it may have been run to look at.
    let registry = registry::load(host).ok().flatten()?;
    let Active::Settled(email) = registry.active() else {
        return None;
    };
    let quarantine = registry.account(email)?.quarantine.as_ref()?;
    Some(unusable(&format!(
        "{email} is the active Account and it is Quarantined: {}.",
        quarantine.because()
    )))
}

/// Where the three files are, said the same way whether or not an agent was
/// launched: with none, this is the whole of what a Triage produced.
fn written(at: &Path) -> Vec<String> {
    vec![
        "What Perch can see of this machine is written down:".to_string(),
        format!("  {}", at.join(PROMPT).display()),
        format!("  {}", at.join(RAW).display()),
        format!("  {}", at.join(REDACTED).display()),
        String::new(),
        format!(
            "Paste {PROMPT} into any coding agent to run the triage by hand, or \
             open an issue at {ISSUES}."
        ),
    ]
}

/// Hands the terminal over, and reports what Claude Code exited with.
///
/// One argument rather than the playbook itself: a `.cmd` shim on Windows runs
/// through `cmd.exe`, which will not carry a multi-kilobyte multiline word.
fn launch(host: &dyn Host, args: &TriageArgs, at: &Path, out: &mut dyn Write) -> Result<i32> {
    let claude = probe::claude_bin(host)?;
    let mut handed: Vec<String> = Vec::new();
    if let Some(model) = &args.model {
        handed.push("--model".to_string());
        handed.push(model.clone());
    }
    handed.push(format!(
        "Read the file \"{}\" and follow its instructions exactly. It is your \
         Perch triage playbook, and it starts by asking what went wrong.",
        at.join(PROMPT).display()
    ));

    host.note(&format!(
        "What Perch can see of this machine is at {}. Starting Claude Code, \
         which will ask what went wrong.",
        at.display()
    ));
    // Before the terminal goes, for the reason a Run flushes: what an earlier
    // command left buffered would otherwise arrive after the session it announced.
    out.flush().map_err(say::failed)?;

    let handed: Vec<&str> = handed.iter().map(String::as_str).collect();
    // No `CLAUDE_CONFIG_DIR`: a Triage launches whatever Claude Code the machine
    // would launch on its own, because pointing one at a Profile is a Run.
    host.exec_interactive(&claude.to_string_lossy(), &handed, &[])
        .map_err(|err| PerchError::Other(format!("could not launch {}: {err}", claude.display())))
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
