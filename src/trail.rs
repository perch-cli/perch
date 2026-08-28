//! What this Perch was asked and what it decided (ADR a-trail-is-evidence).
//!
//! Two lines an invocation — one when the command starts, one when it ends,
//! paired by an `id` — appended and never rewritten. Every write here fails in
//! silence, which is the one place in Perch that does: a diary that can refuse
//! `perch switch` is a new way for every command in the product to fail.
//!
//! A reader skips what it cannot parse rather than refusing it. That is what
//! keeps the Trail out of the Holdings: a shape that degrades needs no version,
//! and needs no migration when it moves.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::holdings;
use crate::host::Host;

/// The live file, and where the cap moves it aside to. Two files taking turns
/// rather than a trim: rotating is one `rename`, and trimming is a rewrite
/// under a lock on the hot path of every command.
const TRAIL: &str = "trail.log";
const ROTATED: &str = "trail.log.1";

/// What the live file may reach before it is moved aside — thousands of
/// invocations, which is months for a person and days for a shell prompt.
const CAP_BYTES: u64 = 1024 * 1024;

/// How far back a Probe reads, before the last failure widens it.
const WINDOW_MINUTES: i64 = 60;

/// Which of an invocation's two lines this is, or the one line a Watcher writes.
///
/// A Watcher round is not an invocation: it has nothing to end, and there is no
/// exit code to carry. It records only the rounds where it moved something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    Start,
    End,
    Acted,
}

/// One line of the Trail.
///
/// Every field but `id`, `at` and `event` has a default, and a reader ignores
/// keys it does not know: a line written by a newer Perch is read by an older
/// one, minus whatever it has no field for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    /// What pairs a start with its end. The start's own moment and the process
    /// that wrote it, which no second invocation on this machine shares.
    pub id: String,
    pub at: DateTime<Utc>,
    pub event: Event,
    #[serde(default)]
    pub perch: String,
    /// Which process wrote it, so a start with no end can be asked whether it is
    /// still running rather than assumed to have died.
    #[serde(default)]
    pub pid: u32,
    /// What was typed, up to `--`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<String>,
    /// How many words went to the client instead of being written down.
    ///
    /// `perch run <target> -- claude -p "..."` hands everything past the
    /// separator to Claude Code, and what somebody types there is theirs. That
    /// anything was given is the invocation's; the words are not.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub passed_on: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

fn is_zero(count: &usize) -> bool {
    *count == 0
}

/// What a command carries from its first line to its last.
pub struct Invocation {
    id: String,
    /// Whether the start line landed. An entry is two lines or none, so the
    /// first command on a machine — which creates the home the Trail lives in —
    /// records nothing rather than an end with no beginning.
    began: bool,
}

/// Writes the start line and answers the handle its end needs.
///
/// Takes what was typed rather than reading the arguments itself, so the words
/// a test gives are the words the line holds.
pub fn began(host: &dyn Host, typed: &[String]) -> Invocation {
    let at = host.now();
    let id = format!("{:x}-{}", at.timestamp_millis(), host.process_id());
    let (words, passed_on) = up_to_the_separator(typed);

    let began = append(
        host,
        &Line {
            id: id.clone(),
            at,
            event: Event::Start,
            perch: env!("CARGO_PKG_VERSION").to_owned(),
            pid: host.process_id(),
            words,
            passed_on,
            exit_code: None,
        },
    );
    Invocation { id, began }
}

/// Writes the end line. A start with none is a command that hung or was killed,
/// which is a finding rather than a gap.
pub fn ended(host: &dyn Host, invocation: &Invocation, exit_code: i32) {
    if !invocation.began {
        return;
    }
    append(
        host,
        &Line {
            id: invocation.id.clone(),
            at: host.now(),
            event: Event::End,
            perch: env!("CARGO_PKG_VERSION").to_owned(),
            pid: host.process_id(),
            words: Vec::new(),
            passed_on: 0,
            exit_code: Some(exit_code),
        },
    );
}

/// Writes the one line a Watcher round leaves, for a round that moved something.
///
/// A round that looked and did nothing is most of them, and recording those
/// would push everything a person typed out of the file within days.
pub fn acted(host: &dyn Host, words: &[String]) {
    let at = host.now();
    append(
        host,
        &Line {
            id: format!("{:x}-{}", at.timestamp_millis(), host.process_id()),
            at,
            event: Event::Acted,
            perch: env!("CARGO_PKG_VERSION").to_owned(),
            pid: host.process_id(),
            words: words.to_vec(),
            passed_on: 0,
            exit_code: None,
        },
    );
}

/// The words before `--`, and how many came after it.
fn up_to_the_separator(typed: &[String]) -> (Vec<String>, usize) {
    match typed.iter().position(|word| word == "--") {
        Some(at) => (typed[..at].to_vec(), typed.len() - at - 1),
        None => (typed.to_vec(), 0),
    }
}

/// Adds one line, moves the file aside once it has grown past the cap, and says
/// whether the line landed.
fn append(host: &dyn Host, line: &Line) -> bool {
    let Ok(home) = holdings::perch_home(host) else {
        return false;
    };
    // Written where Perch already lives, never making the place it lives. A
    // machine Perch holds nothing on has nothing to record, and a Purge that
    // has just emptied one must not find a directory behind it.
    if !host.path_exists(&home) {
        return false;
    }
    let Ok(text) = serde_json::to_string(line) else {
        return false;
    };
    let at = home.join(TRAIL);
    let Ok(width) = host.append_private_line(&at, &text) else {
        return false;
    };
    if width >= CAP_BYTES {
        let _ = host.rename(&at, &home.join(ROTATED));
    }
    true
}

/// What the Trail holds, as a Probe renders it.
pub struct Reading {
    /// Within the window, oldest first.
    pub lines: Vec<Line>,
    /// Every line both files hold, however old.
    pub held: usize,
    pub last_written: Option<DateTime<Utc>>,
    /// Starts within the window whose process is gone: a command that died
    /// without a word. A start whose process is still alive is a command still
    /// running — `perch watcher run` is one on every machine with a Service —
    /// and is not one of these.
    pub unfinished: Vec<Line>,
}

/// Whether the process that wrote a line is the one still holding it.
///
/// The start time tells a live command from a pid the machine handed on, the
/// reasoning a session marker uses (ADR a-profile-is-live-by-evidence). Doubt
/// answers yes, so nothing is called dead on a guess.
fn still_running(host: &dyn Host, line: &Line) -> bool {
    line.pid == 0
        || (host.process_alive(line.pid)
            && host
                .process_started_at(line.pid)
                .is_none_or(|started| started <= line.at))
}

/// Reads both files, newest last.
///
/// The window is the last hour *and* everything since the last non-zero exit:
/// the hour is a guess at what is relevant and the failure is what is actually
/// relevant.
pub fn read(host: &dyn Host) -> Reading {
    let Ok(home) = holdings::perch_home(host) else {
        return Reading {
            lines: Vec::new(),
            held: 0,
            last_written: None,
            unfinished: Vec::new(),
        };
    };

    let mut held: Vec<Line> = Vec::new();
    for file in [ROTATED, TRAIL] {
        let Ok(text) = host.read_file(&home.join(file)) else {
            continue;
        };
        held.extend(
            text.lines()
                .filter_map(|line| serde_json::from_str::<Line>(line).ok()),
        );
    }
    held.sort_by_key(|line| line.at);

    let last_written = held.last().map(|line| line.at);
    // The *start* of the invocation that failed rather than the end line
    // carrying the code: a window that took the end alone would widen to reach
    // half of it, and what somebody ran is on the other half.
    let failed = held
        .iter()
        .rfind(|line| line.exit_code.is_some_and(|code| code != 0))
        .map(|ended| {
            held.iter()
                .find(|line| line.id == ended.id)
                .map_or(ended.at, |began| began.at)
        });
    let from = match (host.now() - Duration::minutes(WINDOW_MINUTES), failed) {
        (window, Some(failed)) if failed < window => failed,
        (window, _) => window,
    };

    let ended: std::collections::BTreeSet<&str> = held
        .iter()
        .filter(|line| line.event == Event::End)
        .map(|line| line.id.as_str())
        .collect();
    // Within the window, because a start left unpaired by a machine going down
    // is one a reboot explains, and one of those would otherwise be reported
    // for as long as the file holds it.
    let unfinished: Vec<Line> = held
        .iter()
        .filter(|line| line.at >= from)
        .filter(|line| line.event == Event::Start && !ended.contains(line.id.as_str()))
        .filter(|line| !still_running(host, line))
        .cloned()
        .collect();

    Reading {
        lines: held
            .iter()
            .filter(|line| line.at >= from)
            .cloned()
            .collect(),
        held: held.len(),
        last_written,
        unfinished,
    }
}
