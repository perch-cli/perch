//! Whether a client is running against a config directory, and what that means
//! for what the caller is about to do (ADR a-profile-is-live-by-evidence).
//!
//! Above `probe`, which holds the Marker's shape because Claude Code invented it,
//! and above `registry`, which holds the document: the five-second NTP margin,
//! doubt resolving towards Live, and what a refusal says are judgments Perch
//! makes rather than fields it reads (ADR code-lives-where-it-reaches).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError};
use crate::probe::{self, Installed};
use crate::registry::Account;

/// A config directory the ask covers, and what a refusal about it calls it.
///
/// Two fields rather than a pair, because every caller supplies both and a
/// `(&str, &Path)` lets them cross.
pub struct Place {
    whose: String,
    dir: PathBuf,
}

impl Place {
    /// A directory the caller has a name for — an Account's Profile, the Default
    /// Profile, a Profile an Import is about to write into.
    pub fn new(whose: impl Into<String>, dir: impl Into<PathBuf>) -> Place {
        Place {
            whose: whose.into(),
            dir: dir.into(),
        }
    }

    /// A directory named by its own path, for the caller with nothing better to
    /// call it: a login Perch is driving, a Profile a Run is Carrying into, a
    /// Credential Store a Renewal would replace.
    pub fn at(dir: impl Into<PathBuf>) -> Place {
        let dir = dir.into();
        Place::new(dir.display().to_string(), dir)
    }

    /// An Account's own Profile. Fails where the address has no character a
    /// directory can be named after, which is a question about the registry
    /// rather than about liveness and so reaches the caller as itself.
    pub fn of_the_profile(host: &dyn Host, account: &Account) -> Result<Place> {
        Ok(Place::new(
            format!("{}'s Profile", account.email()),
            account.profile_dir(host)?,
        ))
    }
}

/// One client running against one of the places asked about.
pub struct Client {
    pub pid: u32,
    /// The place's own name, carried through so a refusal naming several says
    /// which pid is where.
    pub whose: String,
}

/// What the ask came back with.
///
/// Three states behind two arms, and no `From<Answer> for bool`: which way doubt
/// resolves is named at the call site or nowhere.
pub enum Answer {
    Idle(Idle),
    NotIdle(NotIdle),
}

/// The Profile asked about is not a Live Profile: nothing is running against the
/// Credential a write would go under.
///
/// A witness on the terms [`crate::registry::Settled`] sets out. Its field is
/// private, so the only way to hold one is to have matched an [`Answer`].
pub struct Idle(());

/// The two ways it is not Idle. No catch-all arm, so a third breaks the build.
pub enum NotIdle {
    /// A client is running against at least one of the places. The one that
    /// resolves itself: the client exits, and the Credential stops being its.
    Live(Vec<Client>),
    /// Nothing was established, which is not the same as nothing running.
    Unsure(Unsure),
}

/// The processes running against a set of config directories right now.
///
/// A Marker is evidence only when it is corroborated, and one that cannot be
/// read or understood is no evidence at all: a Profile is Live when something
/// says so.
pub fn ask(host: &dyn Host, places: &[Place]) -> Answer {
    let mut found = Vec::new();
    let mut doubt = None;
    for place in places {
        match clients_in(host, &place.dir) {
            Ok(running) => found.extend(running.into_iter().map(|pid| Client {
                pid,
                whose: place.whose.clone(),
            })),
            // Kept rather than returned: evidence outranks doubt where the set
            // holds both, because a client that can be quit is a refusal the
            // reader can act on and an unreadable directory is one they cannot.
            Err(unsure) => doubt = doubt.or(Some(unsure)),
        }
    }

    match (found.is_empty(), doubt) {
        (false, _) => Answer::NotIdle(NotIdle::Live(found)),
        (true, Some(unsure)) => Answer::NotIdle(NotIdle::Unsure(unsure)),
        (true, None) => Answer::Idle(Idle(())),
    }
}

impl Answer {
    /// Whether a caller that only declines rather than refusing should decline.
    /// Doubt counts as a client for that purpose: the Carry writes into a Profile
    /// only when it is quiet.
    /// The witness, or the refusal — for the caller that hands a [`PerchError`]
    /// on rather than deciding between the two ways it was not Idle.
    pub fn idle_or(self, installed: &Installed, consequence: &Consequence) -> Result<Idle> {
        match self {
            Answer::Idle(idle) => Ok(idle),
            Answer::NotIdle(not_idle) => Err(not_idle.refusal(installed, consequence)),
        }
    }

    pub fn counts_as_live(&self) -> bool {
        self.counts_as_live_but(None)
    }

    /// The same, discounting one process — which is only ever the caller's own. A
    /// Run claims its Profile before it reconciles and Carries, and that claim is
    /// a Marker, so the Carry that follows would find it and decline to write. A
    /// reading rather than a parameter of the ask, because any *other* client is
    /// still a client and doubt still resolves towards Live.
    pub fn counts_as_live_but(&self, mine: Option<u32>) -> bool {
        match self {
            Answer::Idle(_) => false,
            Answer::NotIdle(NotIdle::Unsure(_)) => true,
            Answer::NotIdle(NotIdle::Live(clients)) => {
                clients.iter().any(|client| Some(client.pid) != mine)
            }
        }
    }
}

impl NotIdle {
    /// The refusal: what the evidence says, then what the caller says about
    /// itself.
    ///
    /// A doubt keeps only the promise: its own refusal names the broken assumption
    /// and says what to do about the directory (ADR an-assumption-is-probed).
    pub fn refusal(self, installed: &Installed, consequence: &Consequence) -> PerchError {
        let Consequence {
            nothing_happened,
            quit_it,
        } = consequence;
        match self {
            NotIdle::Live(clients) => PerchError::ProfileLive(format!(
                "A client is running against {}.\n{nothing_happened} {quit_it}",
                clause(&clients)
            )),
            NotIdle::Unsure(unsure) => unsure.refusal(installed).with_note(nothing_happened),
        }
    }
}

/// What a caller says about itself when the ask does not come back Idle.
///
/// Two sentences rather than one, because only the first is true either way: a
/// doubt has no client to name and no session to quit, and a refusal that leaves
/// out what did not happen is not one (ADR a-refusal-is-a-promise).
pub struct Consequence {
    /// What did not happen. Said whichever way the ask failed.
    pub nothing_happened: &'static str,
    /// What to do about the client, for the refusal that has one to point at.
    pub quit_it: &'static str,
}

/// What a Switch, a repair, a removal and a watched round all say: they leave
/// exactly nothing behind, and they offer the same two ways out.
pub const NOTHING_WAS_CHANGED: Consequence = Consequence {
    nothing_happened: "Nothing was changed.",
    quit_it: "That Credential belongs to it until it exits. Quit it, or switch \
              to a different Account.",
};

/// Which clients, and where — the opening every refusal about a Live Profile
/// shares. Grouped by place in the order they were asked about, because a reader
/// with two Profiles named at them has to know which to quit.
pub fn clause(clients: &[Client]) -> String {
    let mut places: Vec<(&str, Vec<String>)> = Vec::new();
    for client in clients {
        match places.iter_mut().find(|(whose, _)| *whose == client.whose) {
            Some((_, pids)) => pids.push(client.pid.to_string()),
            None => places.push((&client.whose, vec![client.pid.to_string()])),
        }
    }

    places
        .iter()
        .map(|(whose, pids)| format!("{whose} (pid {})", pids.join(", ")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Why whether anything is running went unanswered. Both are doubt rather than
/// an answer, and neither is decided here: a caller that must not write under a
/// client reads either as one, and the caller that can name a Claude Code
/// version turns either into a refusal that says which it met.
pub enum Unsure {
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
    /// The refusal a doubt makes: the assumption it broke, what it met, and the
    /// Claude Code that was installed when it did.
    pub fn refusal(&self, installed: &Installed) -> PerchError {
        probe::refusal(
            probe::assumption::SESSION_MARKER,
            &self.detail(),
            installed.version(),
        )
    }

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
                 Make that directory readable, or delete it if no client is \
                 running",
                dir.display()
            ),
        }
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

    // Once rather than per Marker: a machine does not reboot between two files.
    let booted = host.booted_at();
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

        // A session does not outlive a reboot, so this Marker names no client
        // whatever its pid is now doing.
        if written_before_the_boot(booted, session_began) {
            continue;
        }

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

/// Whether something recorded at `began` was written before this machine
/// started, and so by a process that did not survive the boot whatever pid it
/// named. The dismissal that needs no permission over the process, so it still
/// answers where the operating system will not say when that process began.
/// `booted_at` is passed in, so a caller reading a set asks the machine once.
pub fn written_before_the_boot(booted_at: Option<DateTime<Utc>>, began: i64) -> bool {
    booted_at.is_some_and(|boot| {
        began.saturating_add(CLOCK_STEP_MARGIN_MILLIS) < boot.timestamp_millis()
    })
}

/// How far a Marker may appear to predate the process it names, or the boot it
/// was written after, and still be taken as evidence. Both figures come off a
/// `btime` the kernel derives as realtime minus uptime on Linux, so an NTP
/// correction makes a live process look younger than the session it just
/// recorded and a boot look later than a Marker that preceded it.
const CLOCK_STEP_MARGIN_MILLIS: i64 = 5_000;

/// Refuses to write into a Profile a client is holding, over the one or two this
/// command writes; `also_the_default_profile` is why that one joins them. One
/// function rather than one per command: each asks twice, and two spellings of
/// one pair of checks is how the second ask comes to be weaker than the first.
pub fn refuse_while_anything_is_running(
    host: &dyn Host,
    account: &crate::registry::Account,
    also_the_default_profile: Option<&'static str>,
    installed: &crate::probe::Installed,
) -> Result<()> {
    let mut places = vec![Place::of_the_profile(host, account)?];
    if let Some(why) = also_the_default_profile {
        // Its Credential is the one a running client is holding, and this would
        // replace it rather than renew it.
        places.push(Place::new(
            why,
            crate::holdings::the_default_profile(host)?.config_dir,
        ));
    }

    ask(host, &places).idle_or(installed, &NOTHING_WAS_CHANGED)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::host::{FakeHost, Platform};
    use crate::probe::Identity;

    /// Midday on an ordinary day, as the epoch milliseconds a Marker records.
    const NOON: i64 = 1_754_308_800_000;

    fn live(host: &dyn Host, dir: &Path) -> bool {
        ask(host, &[Place::at(dir)]).counts_as_live()
    }

    fn moment(millis: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(millis).expect("a time")
    }

    /// Asserted through the ask rather than by reaching for the marker path,
    /// because a check on the path is a check past the interface. The fake
    /// reports its own process as running, which is the situation being modeled:
    /// Perch waits for what it started, so the pid a claim names is alive for
    /// precisely as long as the Run or the login.
    #[test]
    fn a_claim_makes_a_directory_live_and_letting_it_go_stops_it() {
        let dir = Path::new("/Users/someone/.perch/profiles/someone-example-com");
        let host = FakeHost::new();

        assert!(!live(&host, dir), "nothing has claimed it yet");

        let claimed = probe::claim(&host, dir).expect("the marker is written");
        assert!(
            live(&host, dir),
            "a Run or a login holding this is a Live Profile"
        );

        drop(claimed);
        assert!(
            !live(&host, dir),
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

    /// A Marker written before the machine started, wearing a pid a system
    /// daemon took at boot and will not say the start of. Every platform the
    /// fake can claim, because a dismissal that holds on one and not the others
    /// puts the reader back in the refusal on the other two.
    #[test]
    fn a_marker_older_than_the_boot_is_no_evidence_whatever_its_pid_is_doing() {
        for platform in [Platform::MacOs, Platform::Windows, Platform::Other] {
            let host = FakeHost::new()
                .with_platform(platform)
                .with_file(
                    "/tmp/profile/sessions/532.json",
                    &format!(r#"{{"startedAt":{NOON}}}"#),
                )
                .with_live_process_of_unknown_start(532)
                .with_booted_at(moment(NOON + 65 * 60 * 1_000));

            assert_eq!(
                clients_in(&host, Path::new("/tmp/profile")).ok(),
                Some(vec![]),
                "{platform:?}: no session survives a reboot, so the pid this \
                 marker names is one the boot handed out again"
            );
        }
    }

    /// The other half of the same rule, and the one the dismissal must not
    /// swallow: inside one boot an unreadable start establishes nothing.
    #[test]
    fn a_marker_written_after_the_boot_is_the_doubt_it_always_was() {
        let host = FakeHost::new()
            .with_file(
                "/tmp/profile/sessions/532.json",
                &format!(r#"{{"startedAt":{NOON}}}"#),
            )
            .with_live_process_of_unknown_start(532)
            .with_booted_at(moment(NOON - 60 * 60 * 1_000));

        assert!(
            matches!(
                clients_in(&host, Path::new("/tmp/profile")),
                Err(Unsure::WhenItBegan(_))
            ),
            "a live pid with no readable start, inside this boot, still resolves \
             towards Live"
        );
    }

    /// The margin the process comparison already allows, allowed here for the
    /// same reason: Linux derives the boot from a wall clock that steps.
    #[test]
    fn a_marker_a_moment_older_than_the_boot_is_not_dismissed_over_a_clock_step() {
        let host = FakeHost::new()
            .with_file(
                "/tmp/profile/sessions/532.json",
                &format!(r#"{{"startedAt":{NOON}}}"#),
            )
            .with_live_process_of_unknown_start(532)
            .with_booted_at(moment(NOON + CLOCK_STEP_MARGIN_MILLIS - 1));

        assert!(
            clients_in(&host, Path::new("/tmp/profile")).is_err(),
            "two seconds of NTP correction must not dismiss the marker of a \
             client that is running"
        );
    }

    /// A machine with no boot to read dismisses nothing, which is the platform
    /// Perch has no way to ask and the refusal that was there before.
    #[test]
    fn a_machine_that_will_not_say_when_it_booted_dismisses_no_marker() {
        let host = FakeHost::new()
            .that_will_not_say_when_it_booted()
            .with_file(
                "/tmp/profile/sessions/532.json",
                &format!(r#"{{"startedAt":{NOON}}}"#),
            )
            .with_live_process_of_unknown_start(532);

        assert!(clients_in(&host, Path::new("/tmp/profile")).is_err());
    }

    /// Not a Live Profile and not a refusal: nothing about that Profile was ever
    /// established, because there is nowhere to ask about. It reaches the caller
    /// as the registry question it is rather than as a liveness answer.
    #[test]
    fn an_address_no_profile_can_be_named_after_has_nowhere_to_ask_about() {
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

        let nowhere = Place::of_the_profile(&host, &nameless)
            .err()
            .expect("there is nowhere to ask about");

        assert_eq!(nowhere.exit_code(), crate::error::EXIT_INVALID);
    }
}
