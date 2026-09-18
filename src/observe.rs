//! Observing Utilization: what `--refresh` does, and what it will not do to get a
//! figure.
//!
//! The one place that spends network budget, where every other surface renders from
//! cache (ADR a-figure-carries-its-age). A Credential is only Renewed where nothing is
//! holding it (ADR a-profile-is-live-by-evidence), and a Rotation goes back into the
//! Profile it came from under the locks a Switch takes
//! (ADR a-switch-is-written-down-first). Each Account is attempted on its own, and this
//! is where Perch finds out that one is beyond repair.

use std::io::Write;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::registry::WindowUtilization;
type QuotaWindows = Vec<WindowUtilization>;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::lock::{self, Held};
use crate::lock::{Lost, StillOurs};
use crate::name;
use crate::registry::{self, Account, CachedUtilization, Quarantine, Registry};
use crate::say;

/// Whose the allowance a refresh spends is (ADR a-watcher-knob-is-arithmetic).
///
/// Named at the call rather than worked out here: the Watcher holds the watch
/// itself, and a lock cannot say whether the caller is its holder. The ask rides
/// in the arm that can lose the watch, so the role and the ask cannot come apart.
pub enum Spending<'a> {
    /// A Watcher's own round. It is the one pacing this Account, so nothing
    /// stands between it and the read — and the watch it holds is asked between
    /// turns, because a burst can outlast it.
    ItsOwn { still_ours: StillOurs<'a> },
    /// A command somebody typed. Where a Watcher is running, the active
    /// Account's figure is already being kept at the Watcher's interval, and a
    /// second reader spends what the Watcher decides with. It holds the Registry
    /// lock and nothing more, so there is nothing to lose part way.
    BesideTheWatcher,
}

/// Whether this read is one the Watcher has already made: it holds the
/// watch, this is the Account it Refreshes, and what it last read is younger
/// than the interval it reads at. Asked only beside a Watcher: a Watcher's
/// own read is never one it has already made.
fn already_read(host: &dyn Host, registry: &Registry, email: &str) -> bool {
    let Some(account) = registry.account(email) else {
        return false;
    };
    if !registry
        .active_for(account.provider())
        .whose()
        .is_some_and(|on| name::same_name(on, email))
    {
        return false;
    }
    let Some(observed) = registry
        .account(email)
        .and_then(Account::observed_utilization)
    else {
        return false;
    };
    if !crate::watch::figure_stands(observed, host.now()) {
        return false;
    }
    // A lock that cannot be asked about is no Watcher: refusing a read over a
    // question Perch could not answer is the worse of the two mistakes.
    holdings::watcher_lock_spec(host)
        .ok()
        .and_then(|watch| lock::is_held(host, &watch))
        .unwrap_or(false)
}

/// How one Account's turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Fresh figures, now in the cache.
    Observed,
    /// The hourly budget for this Account is spent, so the cache still answers.
    Throttled,
    /// Nothing was read, and this is why. `spent` is whether the attempt asked
    /// Anthropic anything before it stopped: a Back-off paces questions nobody
    /// is answering, and a refusal made before the first request asked none.
    Failed { why: String, spent: bool },
    /// The Account's Credential cannot be used and cannot be recovered from anything
    /// Perch holds, so it is Quarantined. Distinct from a failure because trying again
    /// is not the answer and never will be. `detail` carries whatever the failure
    /// underneath said.
    Quarantined {
        why: Quarantine,
        detail: Option<String>,
    },
    /// The watch went while this reading was in the middle of one. Not a failure
    /// and not a refusal: nothing was read because nothing more was allowed to
    /// be, and the round says it stopped rather than pacing a Back-off.
    Stopped(Lost),
    /// A Watcher read this Account less than one of its intervals ago, so the
    /// cache holds what a request would have returned and the allowance is left
    /// to the reader that has to decide on it.
    JustRead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub email: String,
    /// The Account as the user names it, for the notes below.
    ///
    /// Carried rather than derived, because an `Attempt` has no Registry and the
    /// surfaces that render one show no Accounts: a Watcher's decision line is the only
    /// sentence about that Account on the screen.
    pub named: String,
    pub outcome: Outcome,
}

/// What a refresh did, Account by Account.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub attempts: Vec<Attempt>,
    /// Set when figures were read but could not be kept for next time.
    pub not_kept: Option<String>,
    /// Whether a refresh was asked for, which is not whether one read anything.
    ///
    /// Carried rather than read off `attempts` being non-empty: a Scope holding no
    /// Accounts would otherwise answer `"refresh": null`, which is this document's word
    /// for *nobody asked*.
    pub asked: bool,
    /// Why the reads stopped where they did, for the caller holding the watch.
    ///
    /// Handed back rather than swallowed: a round that stopped read fewer Accounts
    /// than it was given, and read as a reading that failed it would pace a Back-off
    /// off a question nobody was asked.
    pub stopped: Option<Lost>,
}

/// One step of an observation: what it produced, or the outcome that stopped it there.
pub(crate) type Step<T> = std::result::Result<T, Outcome>;

/// Anything that goes wrong away from Anthropic — an unreadable keychain, a lock nobody
/// gave back — is an Account Perch could not read, said in the words that failure
/// already used.
impl From<PerchError> for Outcome {
    fn from(error: PerchError) -> Outcome {
        Outcome::Failed {
            why: error.to_string(),
            spent: false,
        }
    }
}

impl Attempt {
    /// What is worth saying out loud about this Account, if anything. Nothing for one
    /// that was read: the figure says so itself, by carrying an age of "just now".
    fn note(&self) -> Option<String> {
        match &self.outcome {
            Outcome::Observed => None,
            Outcome::Throttled | Outcome::Failed { .. } => self.why_unread(),
            Outcome::JustRead => Some(format!(
                "{}: the Watcher read it less than {} ago.",
                self.named,
                crate::watch::how_often(),
            )),
            // `refresh` breaks on this rather than recording it, so no `Attempt`
            // carries one; the arm is here because the type allows it.
            Outcome::Stopped(_) => None,
            // The Account as the user names it, and the raw address as the Target to
            // type: `perch relogin someone@example.com (as `work`)` is not a command.
            Outcome::Quarantined { why, detail } => {
                Some(why.said_of(&self.named, &self.email, detail.as_deref()))
            }
        }
    }

    /// The same, for a reader who will not use the cached figure: the reason, and no
    /// word about what the cache holds.
    fn why_unread(&self) -> Option<String> {
        match &self.outcome {
            // Names itself, with the raw address as the Target to type.
            Outcome::Quarantined { .. } => self.reason_unread(),
            Outcome::Observed
            | Outcome::Throttled
            | Outcome::JustRead
            | Outcome::Failed { .. }
            | Outcome::Stopped(_) => self
                .reason_unread()
                .map(|reason| format!("{}: {reason}", self.named)),
        }
    }

    /// Why this Account was not read, for a sentence that has already named it.
    pub fn reason_unread(&self) -> Option<String> {
        match &self.outcome {
            Outcome::Observed | Outcome::JustRead | Outcome::Stopped(_) => None,
            Outcome::Throttled => Some(
                "The provider is rate-limiting reads of this Account's utilization.".to_string(),
            ),
            Outcome::Failed { why, .. } => Some(with_a_stop(why)),
            Outcome::Quarantined { why, detail } => {
                Some(why.said_of(&self.named, &self.email, detail.as_deref()))
            }
        }
    }

    /// The same, for a surface that is about to print the Quarantine itself.
    ///
    /// A Quarantined Account carries the reason and the repair on its own line there,
    /// so the note above would say it twice on one screen. What is left is what that
    /// line cannot carry: the failure underneath.
    fn note_beside_the_account(&self) -> Option<String> {
        match &self.outcome {
            Outcome::Quarantined { detail, .. } => detail
                .as_ref()
                .map(|detail| format!("{}: {detail}", self.named)),
            _ => self.note(),
        }
    }

    /// Two keys under the words they mean: `reason` for the machine, `detail` for what
    /// happened underneath, `null` where there was nothing worth keeping.
    ///
    /// `reason` is `null` for every outcome but a Quarantine, because a Quarantine is
    /// the only one that has one.
    fn document(&self) -> serde_json::Value {
        let (outcome, reason, detail) = match &self.outcome {
            Outcome::Observed => ("observed", None, None),
            Outcome::Throttled => (
                "throttled",
                None,
                Some(
                    "The provider is rate-limiting reads of this Account's utilization".to_string(),
                ),
            ),
            Outcome::Failed { why, .. } => ("failed", None, Some(why.clone())),
            Outcome::Quarantined { why, detail } => {
                ("quarantined", Some(why.as_str()), detail.clone())
            }
            Outcome::Stopped(_) => ("stopped", None, None),
            Outcome::JustRead => ("just_read", None, None),
        };
        json!({
            "id": self.email,
            "outcome": outcome,
            "reason": reason,
            "detail": detail,
        })
    }
}

impl Report {
    /// A refresh that was asked for and read nothing, which is what an empty Scope
    /// produces — distinct from the default, which is nobody asking.
    pub fn asked_for() -> Report {
        Report {
            asked: true,
            ..Report::default()
        }
    }

    /// The lines a person is told, about the figures they are not getting.
    ///
    /// Whole, including the Quarantines: a Watcher prints a decision line and no
    /// Accounts at all, and there the Quarantine is the only thing saying why a
    /// candidate was passed over.
    pub fn notes(&self) -> Vec<String> {
        self.said(Attempt::note)
    }

    /// The same for a Watcher, which decides on nothing it did not just read and so
    /// has no cached figure to point at.
    pub fn unread(&self) -> Vec<String> {
        self.said(Attempt::why_unread)
    }

    pub fn attempt_for(&self, email: &str) -> Option<&Attempt> {
        self.attempts
            .iter()
            .find(|attempt| name::same_name(&attempt.email, email))
    }

    /// Says them, before whatever figures they explain — for a surface that goes on to
    /// show each Account with its Quarantine beside it, which is both of the ones that
    /// write here.
    pub fn write_notes_beside_the_accounts(&self, out: &mut dyn Write) -> Result<()> {
        for note in self.said(Attempt::note_beside_the_account) {
            say::line(out, &note)?;
        }
        Ok(())
    }

    fn said(&self, of: impl Fn(&Attempt) -> Option<String>) -> Vec<String> {
        let mut said: Vec<String> = self.attempts.iter().filter_map(of).collect();
        if let Some(not_kept) = &self.not_kept {
            said.push(not_kept.clone());
        }
        said
    }

    /// The same as a script reads it, and `null` when no refresh was asked for, so
    /// "everything was fine" and "nobody asked" are never the same answer.
    pub fn document(&self) -> serde_json::Value {
        if !self.asked {
            return serde_json::Value::Null;
        }
        let accounts: Vec<_> = self.attempts.iter().map(Attempt::document).collect();
        // Both, as an `Attempt` carries both its `outcome` and its `detail`: the
        // human path names the write error, and a log built from this one would
        // otherwise record that the figures were lost with no way to learn why.
        json!({
            "accounts": accounts,
            "kept": self.not_kept.is_none(),
            "not_kept": self.not_kept,
        })
    }
}

/// Reads current Utilization for each of `emails` and keeps what came back.
pub fn refresh(
    host: &dyn Host,
    perch: &mut Held<'_>,
    registry: &mut Registry,
    emails: &[String],
    spending: Spending<'_>,
) -> Report {
    let mut report = Report::asked_for();
    let mut anything_to_keep = false;

    // The role, split back into its two halves: the internals below know only
    // "may I keep going", which is [`StillOurs`]'s whole vocabulary.
    let mut nothing_to_lose = || Ok(());
    let (beside_the_watcher, still_ours): (bool, StillOurs<'_>) = match spending {
        Spending::ItsOwn { still_ours } => (false, still_ours),
        Spending::BesideTheWatcher => (true, &mut nothing_to_lose),
    };

    for email in emails {
        // A round trip to Anthropic each, so the hold is renewed between them and again
        // inside the turn: one Account's turn is up to six requests bounded at thirty
        // seconds, twice the ninety the Registry hold goes stale in.
        perch.renew();
        // And whatever else the caller holds. Kept here beside the ask every request
        // makes for itself: an Account already Quarantined is answered before the
        // first one, so a burst of them would otherwise ask nothing at all.
        if let Err(lost) = still_ours() {
            report.stopped = Some(lost);
            break;
        }

        // Before the Account is read out, and before the hold below is spent: a read
        // the Watcher has already made is one this command declines to make again,
        // rather than one it makes and throws away.
        if beside_the_watcher && already_read(host, registry, email) {
            report.attempts.push(Attempt {
                named: registry.named_for_the_user(email),
                email: email.clone(),
                outcome: Outcome::JustRead,
            });
            continue;
        }

        let Some(account) = registry.account(email).cloned() else {
            continue;
        };
        let outcome = match observe(host, perch, registry, &account, still_ours) {
            // The same answer as the ask at the top of the turn, reached from
            // inside one: nothing is recorded against the Account, because the
            // round stopped rather than learning anything about it.
            Err(Outcome::Stopped(lost)) => {
                report.stopped = Some(lost);
                break;
            }
            Ok(windows) => {
                keep(registry, email, windows, host.now());
                anything_to_keep = true;
                Outcome::Observed
            }
            Err(outcome) => outcome,
        };
        // A Quarantine found here is written down here: Anthropic has already retired
        // what it retired by the time Perch learns of it, so a reason not recorded is a
        // reason discovered again at a browser round trip.
        if let Outcome::Quarantined { why, .. } = &outcome {
            anything_to_keep |= registry.quarantine(email, *why);
        }
        report.attempts.push(Attempt {
            named: registry.named_for_the_user(email),
            email: email.clone(),
            outcome,
        });
    }

    if anything_to_keep && let Err(error) = registry::save(host, perch, registry) {
        report.not_kept = Some(format!(
            "The figures were read but Perch could not write them to its own \
             record, so the next command will show the ones before them: {error}"
        ));
    }
    report
}

fn observe(
    host: &dyn Host,
    perch: &mut Held<'_>,
    registry: &Registry,
    account: &Account,
    still_ours: StillOurs<'_>,
) -> Step<QuotaWindows> {
    account.provider().adapter().configured(host)?.observe(
        host,
        perch,
        &registry.profile_context(host, account)?,
        &account.profile(host)?,
        still_ours,
    )
}

/// A reason as a sentence a second one can follow: a failure wrapped from elsewhere
/// does not always end in a stop.
fn with_a_stop(why: &str) -> String {
    let why = why.trim_end();
    match why.ends_with(['.', '!', '?']) {
        true => why.to_string(),
        false => format!("{why}."),
    }
}

fn keep(registry: &mut Registry, email: &str, windows: QuotaWindows, at: DateTime<Utc>) {
    if let Some(account) = registry.account_mut(email) {
        account.utilization = Some(CachedUtilization {
            observed_at: at,
            windows,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::AccountStoreFixture as _;

    fn attempt(email: &str, outcome: Outcome) -> Attempt {
        Attempt {
            email: email.to_string(),
            named: email.to_string(),
            outcome,
        }
    }

    /// The Watcher holds the watch across the whole of a burst — one read per
    /// candidate, each bounded only at thirty seconds — while the watch goes stale
    /// in twenty-two and a half minutes. So the hold it hands over is renewed on
    /// the same beat as the Registry's, per Account rather than either side.
    #[test]
    fn whatever_the_caller_holds_is_renewed_once_per_account() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut registry = Registry::default();
        let emails: Vec<String> = ["a@example.com", "b@example.com", "c@example.com"]
            .iter()
            .map(|email| {
                registry.upsert(crate::cycle::tests::account(email, vec![]));
                (*email).to_string()
            })
            .collect();
        let mut perch = holdings::lock(&host).expect("nobody holds it");
        // Every Account fails to be read, which is beside the point: the renewal
        // happens before the first request either way.

        let mut renewals = 0;
        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &emails,
            Spending::ItsOwn {
                still_ours: &mut || {
                    renewals += 1;
                    Ok(())
                },
            },
        );

        assert_eq!(report.attempts.len(), 3, "one turn each");
        assert_eq!(renewals, 3, "and one renewal each");
    }

    /// The ask at the top of a turn is not the last chance to answer it. A turn
    /// is up to six requests at thirty seconds, and `store_it` follows a Rotation
    /// that has already retired the refresh token Perch holds — so a stop
    /// answered only at the edges is one answered after the point of no return.
    #[test]
    fn a_watcher_asked_to_stop_mid_turn_never_reaches_the_renewal() {
        const EXPIRED: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","refreshToken":"sk-ant-ort01-spent","expiresAt":1}}"#;

        let host = crate::host::FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_env("HOME", "/Users/someone")
            .with_env("USER", "someone");
        let mut registry = Registry::default();
        let email = "a@example.com";
        registry.upsert(crate::cycle::tests::account(email, vec![]));
        let store = registry
            .account(email)
            .expect("it was just added")
            .store(&host)
            .expect("home is known");
        let [primary, _] = crate::claude_fixture::stores_for(&host, &store);
        primary.write(&host, EXPIRED).expect("the store takes it");

        let mut perch = holdings::lock(&host).expect("nobody holds it");

        let mut asked = 0;
        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &[email.to_string()],
            // The turn is entered, and the stop lands inside it.
            Spending::ItsOwn {
                still_ours: &mut || {
                    asked += 1;
                    match asked {
                        1 => Ok(()),
                        _ => Err(Lost::Stopped),
                    }
                },
            },
        );

        assert!(
            host.sent_to("https://console.anthropic.com/v1/oauth/token")
                .is_empty(),
            "no refresh token was spent by a round that had been told to stop"
        );
        assert_eq!(
            report.stopped,
            Some(Lost::Stopped),
            "and the round says it stopped rather than reporting a failed reading"
        );
        assert!(
            report.attempts.is_empty(),
            "with nothing recorded against the Account: {:?}",
            report.attempts
        );
    }

    /// The ask that guards work sending no request keeps its site. An Account
    /// already Quarantined is answered before the first request, so a burst of
    /// them reaches the caller's choice having asked nothing — and the choice is
    /// the second Watcher's to make.
    #[test]
    fn a_burst_of_quarantined_accounts_still_asks_whether_it_may_go_on() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut registry = Registry::default();
        let emails: Vec<String> = ["a@example.com", "b@example.com", "c@example.com"]
            .iter()
            .map(|email| {
                registry.upsert(crate::cycle::tests::account(email, vec![]));
                registry.quarantine(email, Quarantine::RenewalRejected);
                (*email).to_string()
            })
            .collect();
        let mut perch = holdings::lock(&host).expect("nobody holds it");

        let mut asked = 0;
        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &emails,
            Spending::ItsOwn {
                still_ours: &mut || {
                    asked += 1;
                    match asked {
                        1 => Ok(()),
                        _ => Err(Lost::Stopped),
                    }
                },
            },
        );

        assert!(
            host.http_calls().is_empty(),
            "a Quarantined Account is not asked about, which is what leaves the \
             ask nowhere else to be made"
        );
        assert_eq!(
            report.stopped,
            Some(Lost::Stopped),
            "so the burst ends where the stop arrived"
        );
        assert_eq!(
            report.attempts.len(),
            1,
            "rather than walking every Quarantined address to the end: {:?}",
            report.attempts
        );
    }

    /// `refresh` breaks on a stop rather than recording it, so no `Attempt` ever
    /// carries one — the arms exist because the type allows it, and this is what
    /// they would say. A stop is not a line about an Account and not a reason a
    /// figure is missing: the round reports it through `Report::stopped`.
    #[test]
    fn a_stop_is_reported_by_the_round_rather_than_against_an_account() {
        let stopped = attempt("someone@example.com", Outcome::Stopped(Lost::Stopped));

        assert_eq!(stopped.note(), None, "nothing to say about the Account");
        assert_eq!(stopped.document()["outcome"], "stopped");
        assert_eq!(stopped.document()["reason"], serde_json::Value::Null);
    }

    /// The other half of the same beat: the caller answers whether the burst may
    /// go on, and a watch taken over mid-burst ends it there. Read to the end,
    /// the reads spend an hourly allowance that does not refill early, on a
    /// decision the second Watcher is making instead.
    #[test]
    fn a_burst_the_caller_has_stopped_reads_no_further_accounts() {
        for stopped_by in [Lost::HandedOver, Lost::Stopped] {
            let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
            let mut registry = Registry::default();
            let emails: Vec<String> = ["a@example.com", "b@example.com", "c@example.com"]
                .iter()
                .map(|email| {
                    registry.upsert(crate::cycle::tests::account(email, vec![]));
                    (*email).to_string()
                })
                .collect();
            let mut perch = holdings::lock(&host).expect("nobody holds it");

            let mut asked = 0;
            let report = refresh(
                &host,
                &mut perch,
                &mut registry,
                &emails,
                Spending::ItsOwn {
                    still_ours: &mut || {
                        asked += 1;
                        match asked {
                            1 => Ok(()),
                            _ => Err(stopped_by),
                        }
                    },
                },
            );

            assert_eq!(
                report.attempts.len(),
                1,
                "the first Account is read and the burst ends there ({stopped_by:?})"
            );
        }
    }

    /// The `detail` is how, and the reason is what happened. Both travel, with
    /// the raw address as the Target to type.
    #[test]
    fn a_quarantined_account_names_itself_the_reason_and_the_command_that_ends_it() {
        let quarantined = attempt(
            "someone@example.com",
            Outcome::Quarantined {
                why: Quarantine::RenewalRejected,
                detail: Some("Anthropic did not accept the Credential".to_string()),
            },
        );

        let said = quarantined.note().expect("a Quarantine is worth saying");

        assert!(said.contains("someone@example.com"), "{said}");
        assert!(
            said.contains("Anthropic did not accept the Credential"),
            "{said}"
        );
        assert!(said.contains("perch relogin"), "{said}");
    }

    #[test]
    fn a_failure_away_from_anthropic_spent_nothing_and_says_what_that_failure_said() {
        let outcome = Outcome::from(PerchError::Other("the keychain is locked".to_string()));

        assert_eq!(
            outcome,
            Outcome::Failed {
                why: "the keychain is locked".to_string(),
                spent: false,
            }
        );
    }

    /// The Scope a command draws is read before the Registry lock is taken, so
    /// an address in it may name an Account a `perch remove` has since taken out.
    #[test]
    fn an_address_the_registry_no_longer_holds_is_passed_over_rather_than_read() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");
        let mut registry = Registry::default();
        let mut perch = holdings::lock(&host).expect("nobody holds it");

        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &["gone@example.com".to_string()],
            Spending::BesideTheWatcher,
        );

        assert!(report.attempts.is_empty(), "{:?}", report.attempts);
        assert!(host.http_calls().is_empty());
    }

    #[test]
    fn a_figure_that_was_read_needs_no_line_about_it() {
        let report = Report {
            attempts: vec![attempt("someone@example.com", Outcome::Observed)],
            not_kept: None,
            asked: true,
            stopped: None,
        };
        assert!(report.notes().is_empty(), "the age of the figure says it");
        assert_eq!(report.document()["kept"], true);
    }

    #[test]
    fn every_account_that_lost_a_figure_is_named_with_the_reason() {
        let report = Report {
            attempts: vec![
                attempt("someone@example.com", Outcome::Throttled),
                attempt(
                    "overflow@example.com",
                    Outcome::Failed {
                        why: "no token".into(),
                        spent: true,
                    },
                ),
            ],
            not_kept: None,
            asked: true,
            stopped: None,
        };

        let notes = report.notes();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].starts_with("someone@example.com: "), "{notes:?}");
        assert_eq!(notes[1], "overflow@example.com: no token.");
        assert_eq!(
            report.unread(),
            vec![
                "someone@example.com: The provider is rate-limiting reads of this Account's utilization.".to_string(),
                "overflow@example.com: no token.".to_string(),
            ],
            "and a Watcher, which uses no cached figure, gets the reasons alone"
        );
    }

    #[test]
    fn a_refresh_nobody_asked_for_is_not_reported_as_one_that_went_well() {
        let report = Report::default();
        assert!(!report.asked);
        assert_eq!(report.document(), serde_json::Value::Null);
    }

    #[test]
    fn a_refresh_that_had_nothing_to_read_is_not_reported_as_one_nobody_asked_for() {
        let asked = Report::asked_for();

        assert!(asked.asked);
        assert_eq!(
            asked.document(),
            json!({"accounts": [], "kept": true, "not_kept": null}),
            "asked, read nothing, kept nothing to fail at keeping"
        );
    }

    /// The human path names the write error; `--json` said only `false`. A log
    /// built from it recorded that the figures were lost with no way to learn why,
    /// and the two surfaces disagreed about how much they say about one event.
    #[test]
    fn a_refresh_that_could_not_be_kept_says_why_on_both_surfaces() {
        let report = Report {
            attempts: vec![attempt("someone@example.com", Outcome::Observed)],
            not_kept: Some("the registry is read-only".to_string()),
            asked: true,
            stopped: None,
        };

        let document = report.document();
        assert_eq!(document["kept"], false);
        assert_eq!(document["not_kept"], "the registry is read-only");
        assert_eq!(
            report.notes(),
            vec!["the registry is read-only".to_string()],
            "the same sentence the human path prints"
        );
    }
}
