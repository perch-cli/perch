//! Whether a client is running against a config directory, and what that means
//! for what the caller is about to do (ADR a-profile-is-live-by-evidence).
//!
//! Above `probe`, which holds the Marker's shape because Claude Code invented it,
//! and above `registry`, which holds the document: the five-second NTP margin,
//! doubt resolving towards Live, and what a refusal says are judgments Perch
//! makes rather than fields it reads (ADR code-lives-where-it-reaches).

use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::probe::{self, Installed};
use crate::registry::{self, Account};

/// The processes running against a config directory right now. A marker is
/// evidence only when it is corroborated, and one that cannot be read or
/// understood is no evidence at all: a Profile is Live when something says so.
/// One situation is neither belief nor dismissal — a running process whose start
/// the operating system will not say — and that is a refusal.
pub fn live_clients(host: &dyn Host, config_dir: &Path, installed: &Installed) -> Result<Vec<u32>> {
    clients_in(host, config_dir).map_err(|unsure| {
        probe::refusal(
            probe::assumption::SESSION_MARKER,
            &unsure.detail(),
            installed.version(),
        )
    })
}

/// Why whether anything is running went unanswered. Both are doubt rather than
/// an answer, and neither is decided here: a caller that must not write under a
/// client reads either as one, and the caller that can name a Claude Code
/// version turns either into a refusal that says which it met.
enum Unsure {
    /// A marker naming a running process whose start the operating system will
    /// not say, so it can be neither corroborated nor dismissed.
    WhenItBegan(PathBuf),
    /// A marker naming a running process that Perch could not read at all —
    /// root-owned after a `sudo claude`, most often. Its own variant rather than
    /// the one above, because they are told apart by what the reader has to do:
    /// one is a file whose permissions are wrong, and the other is an operating
    /// system that would not answer.
    Unreadable(PathBuf),
    /// The sessions directory is there and would not be read. Told apart from
    /// an absent one, which is the ordinary "nothing is running" and the whole
    /// reason [`Files::list_dir`] reports the two differently.
    Unlistable { dir: PathBuf, why: HostError },
}

impl Unsure {
    fn detail(&self) -> String {
        match self {
            Unsure::WhenItBegan(marker) => format!(
                "{} names a running process, but when that process began could \
                 not be read, so the marker can be neither corroborated nor \
                 dismissed. If that session is dead, delete the file",
                marker.display()
            ),
            Unsure::Unreadable(marker) => format!(
                "{} names a running process and could not be read, so the \
                 marker can be neither corroborated nor dismissed. Make that \
                 file readable, or delete it if that session is dead",
                marker.display()
            ),
            Unsure::Unlistable { dir, why } => format!(
                "{} could not be read ({why}), so whether a client is running \
                 against this Profile is not a question that got an answer. \
                 Nothing is assumed either way — make that directory readable, \
                 or delete it if no client is running",
                dir.display()
            ),
        }
    }
}

/// Whether anything may be running against a config directory, where a marker
/// that can be neither corroborated nor dismissed counts as one. The question
/// [`live_clients`] answers, for the caller with no Claude Code version to name
/// an assumption against: the Carry writes into a Profile only when it is quiet,
/// and doubt is the same answer as a client for that purpose.
pub fn anything_running(host: &dyn Host, config_dir: &Path) -> bool {
    anything_running_but(host, config_dir, None)
}

/// The same, discounting one process — which is only ever the caller's own. A Run
/// claims its Profile before it reconciles and Carries, and that claim is a
/// Marker, so the Carry that follows would find it and decline to write.
/// Discounting rather than skipping the question: any *other* client is still a
/// client, and doubt still resolves towards Live.
pub fn anything_running_but(host: &dyn Host, config_dir: &Path, mine: Option<u32>) -> bool {
    match clients_in(host, config_dir) {
        Ok(running) => running.iter().any(|pid| Some(*pid) != mine),
        Err(_) => true,
    }
}

/// The processes running against a config directory, or the marker that could
/// be neither corroborated nor dismissed. Both callers phrase that doubt in
/// their own terms, and neither decides it.
fn clients_in(host: &dyn Host, config_dir: &Path) -> std::result::Result<Vec<u32>, Unsure> {
    let dir = probe::sessions_dir(config_dir);
    let markers = match host.list_dir(&dir) {
        Ok(markers) => markers,
        // Never having run a client is the *only* case that means nothing is
        // running. A directory that is there and will not be read is doubt, and
        // every doubt in this function resolves towards Live.
        Err(HostError::NotFound { .. }) => return Ok(Vec::new()),
        Err(why) => return Err(Unsure::Unlistable { dir, why }),
    };

    let mut running = Vec::new();
    for marker in markers {
        let pid: u32 = match marker
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|name| name.parse().ok())
        {
            Some(pid) => pid,
            None => continue,
        };
        let session_began = match probe::session_start_in(host, &marker) {
            probe::Marker::Began(at) => at,
            // Either way it does not say what a marker has to say, which is a
            // judgment about the *content* of a file Perch can see all of: a
            // Profile is Live when something says so.
            probe::Marker::SaysNothing => continue,
            // Nothing has been established, so it resolves towards Live — and
            // only for a pid that is running, since litter must not refuse every
            // Switch for ever. One halfway written is `SaysNothing` above.
            probe::Marker::Unreadable if host.process_alive(pid) => {
                return Err(Unsure::Unreadable(marker));
            }
            probe::Marker::Unreadable => continue,
        };

        match host.process_started_at(pid) {
            Some(process_began) => {
                // Saturating, for the reason `usable` is: `startedAt` comes out
                // of a file Perch does not own, and an `i64::MAX` in it would
                // wrap a Live Profile into "nothing running" in a release build.
                if process_began.timestamp_millis()
                    <= session_began.saturating_add(CLOCK_STEP_MARGIN_MILLIS)
                {
                    running.push(pid);
                }
            }
            // No start to compare. The process being gone is the ordinary way
            // that happens — a marker left behind by a client that died.
            None if !host.process_alive(pid) => {}
            None => return Err(Unsure::WhenItBegan(marker)),
        }
    }
    Ok(running)
}

/// How far a process may appear to have begun *after* the session it is named by
/// and still be taken as the one that wrote the Marker. Linux recomputes a
/// process's start from a `btime` the kernel derives as realtime minus uptime,
/// so an NTP correction makes a live process look younger than the session it
/// just recorded.
const CLOCK_STEP_MARGIN_MILLIS: i64 = 5_000;

/// The Profile that was asked about is not a Live Profile: nothing is running
/// against the Credential a write would go under.
///
/// A witness on the terms [`crate::switch::Settled`] sets out — the negative of a **Live
/// Profile**, constructible only by [`refuse_if_live`].
pub struct Idle(());

/// Every way the liveness ask can fail, by name.
///
/// Named one at a time rather than collapsed into a [`PerchError`] because two
/// of the three are not refusals at all, and a caller deciding what to do next
/// has to tell them apart. No catch-all arm, so a fourth breaks the build.
pub enum NotIdle {
    /// A client is running against the Profile, and this is the sentence saying
    /// which. The one that resolves itself: the client exits, and the Credential
    /// stops being its.
    Live(String),
    /// The `sessions` directory is there and would not be read — the root-owned
    /// one a `sudo claude` leaves. Nothing about the Profile was established,
    /// which is not the same as nothing running against it.
    SessionsUnreadable(PerchError),
    /// The Account is recorded under an address no Profile directory can be
    /// named after, so there is nowhere to ask about.
    Unnameable(PerchError),
}

/// For the callers that have nothing to decide off which way it failed, and only
/// want to hand it on — the shape `?` gives them for free.
impl From<NotIdle> for PerchError {
    fn from(not_idle: NotIdle) -> PerchError {
        match not_idle {
            NotIdle::Live(why) => PerchError::ProfileLive(why),
            NotIdle::SessionsUnreadable(error) | NotIdle::Unnameable(error) => error,
        }
    }
}

/// Refuses to touch a Profile something else is holding
/// Public because two callers ask it
/// *before* they spend something rather than after: `perch relogin` before a
/// browser round trip, and `perch watcher run` before it reads every
/// candidate's Utilization.
pub fn refuse_if_live(
    host: &dyn Host,
    account: &Account,
    installed: &Installed,
) -> std::result::Result<Idle, NotIdle> {
    let profile_dir = account.profile_dir(host).map_err(NotIdle::Unnameable)?;
    refuse_if_live_in(
        host,
        &profile_dir,
        &format!("{}'s Profile", account.email()),
        installed,
    )
}

/// The same, of a config directory named rather than derived — the Default
/// Profile, which belongs to no one Account and is where a repair of the Account
/// you are on has to land.
pub fn refuse_if_live_in(
    host: &dyn Host,
    config_dir: &Path,
    whose: &str,
    installed: &Installed,
) -> std::result::Result<Idle, NotIdle> {
    let running = live_clients(host, config_dir, installed).map_err(NotIdle::SessionsUnreadable)?;
    if running.is_empty() {
        return Ok(Idle(()));
    }

    let pids: Vec<String> = running.iter().map(u32::to_string).collect();
    Err(NotIdle::Live(format!(
        "A client is running against {whose} (pid {}).\n\
         Nothing was changed. That Credential belongs to it until it exits — \
         quit it, or switch to a different Account.",
        pids.join(", ")
    )))
}

/// Both Profiles a command may write into, refused while a client is holding
/// either. One place, because every command that needs this asks it twice —
/// before an unbounded wait and after — and two spellings of one pair of checks
/// is how the second ask comes to be weaker than the first. The sentence is the
/// caller's: what makes the Default Profile wrong differs for repair and removal.
pub fn refuse_if_live_anywhere(
    host: &dyn Host,
    account: &Account,
    the_default_profile_too: Option<&str>,
    installed: &Installed,
) -> Result<()> {
    refuse_if_live(host, account, installed)?;

    if let Some(whose) = the_default_profile_too {
        // Its Credential is the one a running client is holding, and this would
        // replace it rather than renew it.
        refuse_if_live_in(
            host,
            &registry::the_default_profile(host)?.config_dir,
            whose,
            installed,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    use crate::host::FakeHost;
    use crate::probe::Identity;

    /// Midday on an ordinary day, as the epoch milliseconds a Marker records.
    const NOON: i64 = 1_754_308_800_000;

    /// Asserted through `anything_running` rather than by reaching for the marker
    /// path, because a check on the path is a check past the interface. The fake
    /// reports its own process as running, which is the situation being modeled:
    /// Perch waits for what it started, so the pid a claim names is alive for
    /// precisely as long as the Run or the login.
    #[test]
    fn a_claim_makes_a_directory_live_and_letting_it_go_stops_it() {
        let dir = Path::new("/Users/someone/.perch/profiles/someone-example-com");
        let host = FakeHost::new();

        assert!(!anything_running(&host, dir), "nothing has claimed it yet");

        let claimed = probe::claim(&host, dir).expect("the marker is written");
        assert!(
            anything_running(&host, dir),
            "a Run or a login holding this is a Live Profile"
        );

        drop(claimed);
        assert!(
            !anything_running(&host, dir),
            "and it stops being Live when the thing holding it lets go"
        );
    }

    /// `startedAt` is a number out of a file Perch does not own, so the margin
    /// added to it is arithmetic on a stranger's input.
    #[test]
    fn a_marker_claiming_the_end_of_time_still_reads_as_a_live_client() {
        let host = FakeHost::new()
            .with_file(
                "/tmp/profile/sessions/4242.json",
                &format!(r#"{{"startedAt":{}}}"#, i64::MAX),
            )
            .with_live_process_started_at(
                4242,
                DateTime::from_timestamp_millis(NOON).expect("a time"),
            );

        assert_eq!(
            clients_in(&host, Path::new("/tmp/profile")).ok(),
            Some(vec![4242]),
            "a process that began long before the marker claims is running against \
             the Profile, whatever the claim adds up to"
        );
    }

    /// The pid is read back out of a filename, so any file in the directory
    /// names one — and `0` is a process group to `kill` and the kernel to
    /// macOS's `proc_pidinfo`, which answers a start time at boot that is
    /// older than every session a marker could record.
    #[test]
    fn a_marker_named_after_a_number_that_is_no_process_makes_nothing_live() {
        let host = FakeHost::new()
            .with_file(
                "/tmp/profile/sessions/0.json",
                &format!(r#"{{"startedAt":{NOON}}}"#),
            )
            .with_live_process_started_at(0, DateTime::<Utc>::MIN_UTC);

        assert_eq!(
            clients_in(&host, Path::new("/tmp/profile")).ok(),
            Some(vec![]),
            "a start time believed here would refuse every Switch, Capture and \
             Renewal against the Profile for ever, and no client could be quit \
             to clear it"
        );
    }

    #[test]
    fn an_address_no_profile_can_be_named_after_is_unnameable_rather_than_idle() {
        let host = FakeHost::new();
        let nameless = Account {
            identity: Identity {
                // Nothing a directory can be named after survives the slug.
                email: "@".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        };

        let not_idle = refuse_if_live(&host, &nameless, &Installed::unknown("2.1.221"))
            .err()
            .expect("there is nowhere to ask about");

        assert!(
            matches!(not_idle, NotIdle::Unnameable(_)),
            "not a Live Profile and not a refusal: nothing about that Profile \
             was ever established"
        );
        assert_eq!(
            PerchError::from(not_idle).exit_code(),
            crate::error::EXIT_INVALID,
            "and it keeps the code the failure earned, rather than being folded \
             into the refusal's",
        );
    }
}
