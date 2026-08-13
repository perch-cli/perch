//! Observing Utilization: what `--refresh` does, and what it will not do to
//! get a figure.
//!
//! Every other surface renders Utilization from cache (ADR 0015). This is the
//! one place that spends network budget, and it spends it carefully: an Account
//! is only asked about with a Credential that is good, a Credential is only
//! renewed where nothing is holding it (ADR 0005), and a Rotation goes back
//! into the Profile it came from under the same locks a Switch takes (ADR
//! 0006).
//!
//! Each Account is attempted on its own. One that cannot be read leaves the
//! others alone and leaves its own cached figure standing, because a display
//! that lost every figure to one broken Account would answer worse than one
//! with a single gap in it.
//!
//! This is also where Perch finds out that an Account is beyond repair, because
//! a Renewal is the only thing that asks a refresh token to prove it still
//! works. A rejection, a Rotation that could not be stored, and a Credential
//! with nothing left to renew with are all terminal: they Quarantine the
//! Account rather than failing an Account that might work next time.

use std::io::Write;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::anthropic::{self, QuotaWindows, Refused};
use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::{self, Held};
use crate::probe::{self, Credential, Installed, Store};
use crate::profile;
use crate::registry::{self, Account, CachedUtilization, Quarantine, Registry};

/// How one Account's turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Fresh figures, now in the cache.
    Observed,
    /// The hourly budget for this Account is spent, so the cache still answers
    /// (ADR 0015).
    Throttled,
    /// Nothing was read, and this is why.
    Failed(String),
    /// The Account's Credential cannot be used and cannot be recovered from
    /// anything Perch holds, so it is Quarantined. Distinct from a failure
    /// because trying again is not the answer and never will be: `detail`
    /// carries whatever the failure underneath said, where there was one worth
    /// keeping.
    Quarantined {
        why: Quarantine,
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub email: String,
    /// The Account as the user names it — by its Alias where it has one — for
    /// the notes below.
    ///
    /// Carried rather than derived, because an `Attempt` has no registry and
    /// the surfaces that render one are the surfaces that show no Accounts:
    /// `perch watch` prints a decision line and nothing else, and the TUI's
    /// State column says an Account is Quarantined without saying why. So this
    /// was the only sentence about that Account on the screen, and it was the
    /// one place in Perch naming an Account by raw address while every other
    /// surface called it ``someone@example.com (as `work`)``.
    pub named: String,
    pub outcome: Outcome,
}

/// What a refresh did, Account by Account.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    pub attempts: Vec<Attempt>,
    /// Set when figures were read but could not be kept for next time.
    pub not_kept: Option<String>,
}

/// One step of an observation: what it produced, or the outcome that stopped
/// it there.
type Step<T> = std::result::Result<T, Outcome>;

/// Anything that goes wrong away from Anthropic — an unreadable keychain, a
/// lock nobody gave back — is an Account Perch could not read, said in the
/// words that failure already used.
impl From<PerchError> for Outcome {
    fn from(error: PerchError) -> Outcome {
        Outcome::Failed(error.to_string())
    }
}

impl Attempt {
    /// What is worth saying out loud about this Account, if anything.
    ///
    /// Nothing for one that was read: the figure says so itself, by carrying an
    /// age of "just now".
    fn note(&self) -> Option<String> {
        match &self.outcome {
            Outcome::Observed => None,
            Outcome::Throttled => Some(format!(
                "{}: {}. The cached figure is what you see.",
                self.named,
                Refused::Throttled
            )),
            Outcome::Failed(why) => Some(format!("{}: {why}", self.named)),
            // The Account as the user names it, and the raw address as the
            // Target to type: `perch relogin someone@example.com (as `work`)`
            // is not a command, and an Alias is not what an Account always has.
            Outcome::Quarantined { why, detail } => {
                Some(why.said_of(&self.named, &self.email, detail.as_deref()))
            }
        }
    }

    /// The same, for a surface that is about to print the Quarantine itself.
    ///
    /// `perch status` and `perch list` show every Account they refreshed, and a
    /// Quarantined one carries the reason and the repair on its own line — so
    /// the note above put the identical sentence, `perch relogin` and all, twice
    /// on one screen. Reached by `perch status --refresh` on any Quarantined
    /// Account, which is not a rare shape: that command is what somebody runs
    /// when an Account has stopped working.
    ///
    /// What is left is the part the Account's own line cannot carry — whatever
    /// the failure underneath said, where there was one worth keeping.
    fn note_beside_the_account(&self) -> Option<String> {
        match &self.outcome {
            Outcome::Quarantined { detail, .. } => detail
                .as_ref()
                .map(|detail| format!("{}: {detail}", self.named)),
            _ => self.note(),
        }
    }

    fn document(&self) -> serde_json::Value {
        let (outcome, detail) = match &self.outcome {
            Outcome::Observed => ("observed", None),
            Outcome::Throttled => ("throttled", Some(Refused::Throttled.to_string())),
            Outcome::Failed(why) => ("failed", Some(why.clone())),
            Outcome::Quarantined { why, .. } => ("quarantined", Some(why.as_str().to_string())),
        };
        json!({"email": self.email, "outcome": outcome, "detail": detail})
    }
}

impl Report {
    /// Whether a refresh was asked for at all.
    pub fn asked(&self) -> bool {
        !self.attempts.is_empty()
    }

    /// The lines a person is told, which are the ones about figures they are
    /// not getting.
    ///
    /// Whole, including the Quarantines. For the surfaces that show the
    /// Accounts themselves that is one sentence too many — see
    /// [`write_notes_beside_the_accounts`] — but `perch watch` prints a decision
    /// line and no Accounts at all, and there the Quarantine is the only thing
    /// saying why a candidate was passed over. The TUI is the same: its State
    /// column says an Account is Quarantined and nothing there says why.
    ///
    /// [`write_notes_beside_the_accounts`]: Report::write_notes_beside_the_accounts
    pub fn notes(&self) -> Vec<String> {
        self.said(Attempt::note)
    }

    /// Says them, before whatever figures they explain — for a surface that goes
    /// on to show each Account with its Quarantine beside it, which is both of
    /// the ones that write here.
    ///
    /// Every surface that renders Utilization has the same thing to say first,
    /// so it is said in one place.
    pub fn write_notes_beside_the_accounts(&self, out: &mut dyn Write) -> Result<()> {
        for note in self.said(Attempt::note_beside_the_account) {
            say(out, &note)?;
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

    /// The same as a script reads it, and `null` when no refresh was asked for
    /// — so "everything was fine" and "nobody asked" are never the same answer.
    pub fn document(&self) -> serde_json::Value {
        if !self.asked() {
            return serde_json::Value::Null;
        }
        let accounts: Vec<_> = self.attempts.iter().map(Attempt::document).collect();
        json!({"accounts": accounts, "kept": self.not_kept.is_none()})
    }
}

/// Reads current Utilization for each of `emails` and keeps what came back.
pub fn refresh(
    host: &dyn Host,
    perch: &mut Held<'_>,
    registry: &mut Registry,
    emails: &[String],
) -> Report {
    let mut report = Report::default();
    let mut anything_to_keep = false;

    for email in emails {
        // A round trip to Anthropic each, over as many Accounts as a Group
        // holds. Said between them rather than only at the write, so a `--group`
        // refresh over a slow connection does not run past the staleness window
        // and hand another `perch` the lock this one is still working under.
        perch.renew();

        let Some(account) = registry.account(email).cloned() else {
            continue;
        };
        let outcome = match observe(host, registry, &account) {
            Ok(windows) => {
                keep(registry, email, windows, host.now());
                anything_to_keep = true;
                Outcome::Observed
            }
            Err(outcome) => outcome,
        };
        // A Quarantine found here is written down here. Anthropic has already
        // retired what it retired by the time Perch learns of it, so a reason
        // not recorded is a reason discovered again — at a browser round trip
        // each time, and read as a fresh mystery each time.
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

fn observe(host: &dyn Host, registry: &Registry, account: &Account) -> Step<QuotaWindows> {
    // An Account already known to be beyond repair is not asked again. Its
    // Credential cannot be renewed and no answer would be recorded against it,
    // so a read here would spend an allowance that does not refill early (ADR
    // 0015) to learn what Perch wrote down last time.
    if let Some(why) = account.quarantine {
        return Err(Outcome::Quarantined { why, detail: None });
    }

    let installed = Installed::probed(host)?;
    let asked = holding(host, registry, account)?;
    let token = usable_token(host, &asked, &installed).map_err(|outcome| {
        only_off_a_credential_that_is_theirs(host, outcome, &asked, account, &installed)
    })?;
    confirm(host, &token, account)?;
    let read = anthropic::utilization(host, &token);
    read.map_err(reading_refused)
}

/// Keeps a Quarantine from being recorded off a Credential that was never
/// established to be this Account's.
///
/// ADR 0019: a figure is recorded only against the Account it was read for. A
/// Quarantine is a recording too, and a terminal one — and the store the active
/// Account is asked about with is the Default Profile, which Perch is not the
/// only thing that writes. Somebody who runs `claude` and logs in directly
/// leaves Perch's record of who is active behind; when that login later dies, a
/// refresh reads *their* dead Credential and would condemn the Account Perch
/// believes is active — whose own Profile still holds a good copy.
///
/// [`confirm`] asks Anthropic whose a token is, but it needs a working token
/// and this is the path where there is none. So the evidence used here is
/// local, and is the machine's own: Claude Code writes the Credential and the
/// Identity beside it together, so a `.claude.json` naming this Account says
/// the Credential there is theirs — the same evidence
/// [`crate::switch::already_landed`] reads.
fn only_off_a_credential_that_is_theirs(
    host: &dyn Host,
    outcome: Outcome,
    asked: &Asked,
    account: &Account,
    installed: &Installed,
) -> Outcome {
    let Outcome::Quarantined { why, detail } = &outcome else {
        return outcome;
    };
    if asked.its_own_profile || names(host, &asked.store, account, installed) {
        return outcome;
    }

    let how = match detail {
        Some(detail) => format!(" ({detail})"),
        None => String::new(),
    };
    Outcome::Failed(format!(
        "the live Credential could not be used — {}{how} — but {} does not \
         name {}, so it may belong to a login made outside Perch and nothing \
         was recorded against this Account. `perch switch {}` puts its own \
         Credential back in place.",
        why.because(),
        asked.store.identity_file.display(),
        account.email(),
        account.email(),
    ))
}

/// Whether a store's Identity names this Account.
fn names(host: &dyn Host, store: &Store, account: &Account, installed: &Installed) -> bool {
    probe::read_identity(host, store, installed)
        .ok()
        .flatten()
        .is_some_and(|identity| registry::same_name(&identity.email, account.email()))
}

/// The store an Account is asked about with, and whose it is.
///
/// The two are inseparable, because an empty store means opposite things in the
/// two cases: an Account's own Profile holding nothing is an Account nothing
/// Perch has can recover, and the Default Profile holding nothing is a Claude
/// Code that is logged out.
struct Asked {
    store: Store,
    /// Whether this is the Account's own Profile rather than the Default one.
    its_own_profile: bool,
    /// Every configuration directory a client could be holding this Account's
    /// Credential from, which is what a Renewal has to be refused against.
    ///
    /// More than the store being renewed, because a Rotation retires the
    /// refresh token for an *Account* rather than for a file: every copy of
    /// that Credential dies together, wherever it is being held from.
    in_use_from: Vec<PathBuf>,
}

/// Which store holds the Credential to ask with.
///
/// For the active Account that is the Default Profile: what is live there is
/// its Credential, and it is ahead of the copy in its own Profile, which only
/// catches up when a Switch away Captures it (ADR 0006). Every other Account is
/// asked about with the Credential in its own Profile.
fn holding(host: &dyn Host, registry: &Registry, account: &Account) -> Result<Asked> {
    let its_own_profile = account.profile_dir(host)?;
    if registry.is_active(account.email()) {
        let store = registry::the_default_profile(host)?;
        // Two directories rather than one, and this is the only case where they
        // differ. The copy being renewed is the live one in the Default
        // Profile, but `perch run <this account>` points a client at the
        // Account's own Profile — and the Rotation that renewal may cause would
        // retire that client's refresh token along with this one (ADR 0027).
        Ok(Asked {
            in_use_from: vec![store.config_dir.clone(), its_own_profile],
            store,
            its_own_profile: false,
        })
    } else {
        Ok(Asked {
            in_use_from: vec![its_own_profile],
            store: account.store(host)?,
            its_own_profile: true,
        })
    }
}

/// An access token that can still be asked a question, renewing the Credential
/// when the one there is has run out.
fn usable_token(host: &dyn Host, asked: &Asked, installed: &Installed) -> Step<String> {
    let credential = credential_in(host, asked, installed)?;
    if credential.usable_at(host.now()) {
        return Ok(credential.access_token);
    }

    // Asked before the locks are taken, so an Account that was never going to
    // be renewed says so without queueing behind anything, and asked again
    // under them, where the answer is the one that counts.
    refuse_if_live(host, asked, installed)?;
    renew_under_the_lock(host, asked, installed)
}

/// Refuses to renew a Credential something else is holding (ADR 0005).
///
/// Anthropic retires the old refresh token when it Rotates one, so renewing a
/// Credential a running Claude Code has in memory logs that session out
/// silently, mid-task. Asked of every directory that Credential could be in use
/// from rather than only the one being written, because a Rotation kills every
/// copy of it at once.
fn refuse_if_live(host: &dyn Host, asked: &Asked, installed: &Installed) -> Step<()> {
    let mut running = Vec::new();
    for config_dir in &asked.in_use_from {
        running.extend(probe::live_clients(host, config_dir, installed)?);
    }
    if running.is_empty() {
        return Ok(());
    }

    let pids: Vec<String> = running.iter().map(u32::to_string).collect();
    Err(Outcome::Failed(format!(
        "its access token has expired and a client is running against that \
         Account (pid {}), so renewing it would log that session out. The \
         cached figure is what you see.",
        pids.join(", ")
    )))
}

/// The Credential to ask with, or what its absence means.
///
/// An Account's own Profile holding nothing is terminal: that Profile is the
/// only place its Credential ever lives, so nothing Perch holds can make it
/// answerable again. The Default Profile holding nothing is not terminal at all
/// — it is a Claude Code that has been logged out, and the Account's own copy is
/// still there to switch back to.
fn credential_in(host: &dyn Host, asked: &Asked, installed: &Installed) -> Step<Credential> {
    probe::read_credential(host, &asked.store, installed)?.ok_or_else(|| {
        if asked.its_own_profile {
            Outcome::Quarantined {
                why: Quarantine::NoCredential,
                detail: None,
            }
        } else {
            Outcome::Failed(
                "the Default Profile holds no Credential, so Claude Code is \
                 logged out and there is nothing to ask Anthropic with."
                    .to_string(),
            )
        }
    })
}

/// Renews the Credential in a store, and puts the Rotation back where it came
/// from before the new token is used for anything.
///
/// Under Claude Code's own locks, in Claude Code's own order (ADR 0006), with
/// Claude Code's own double-checked re-read: whoever was holding the lock while
/// Perch waited for it may have renewed the very Credential Perch was about to.
fn renew_under_the_lock(host: &dyn Host, asked: &Asked, installed: &Installed) -> Step<String> {
    let store = &asked.store;
    lock::under(host, probe::locks_for(store), |held| {
        // Both of the questions asked before the locks were taken, asked again
        // now that nothing can change the answer underneath Perch.
        refuse_if_live(host, asked, installed)?;
        let credential = credential_in(host, asked, installed)?;
        if credential.usable_at(host.now()) {
            return Ok(credential.access_token);
        }

        // An access token that has run out and no refresh token to buy another
        // with is the end of what this Credential can do. Nothing Perch holds
        // renews it and nothing it could ask would change that.
        let refresh_token = credential
            .refresh_token
            .clone()
            .ok_or(Outcome::Quarantined {
                why: Quarantine::NoRefreshToken,
                detail: None,
            })?;

        // Said before the network round trip rather than only after it. The
        // config-file lock goes stale in ten seconds and a request to Anthropic
        // can take longer than that on its own, so a renewal that only happens
        // between steps leaves the longest step of all running under a lock
        // somebody else may take over — and the takeover is then discovered
        // afterwards, when the Rotation has already happened.
        //
        // Claude Code's lock alone, and not Perch's, which is why this is not
        // the [`crate::lock::Holds`] pair a Switch uses. The registry hold is
        // renewed once per Account above and goes stale in ninety seconds
        // rather than ten, and what it is protecting here is the write of the
        // *figures* — a Credential that Rotated is written under this lock, not
        // that one. So a round trip long enough to lose it costs the reading
        // and says so, through `not_kept`, rather than costing a Credential.
        held.renew();

        // Anthropic may Rotate here. Everything after this line is Perch making
        // sure the Rotation is not lost.
        let renewal = anthropic::renew(host, &refresh_token);
        let fresh = renewal.map_err(not_renewed)?;

        held.renew();
        let rotated = probe::credential_after_rotation(
            &credential,
            &fresh.access_token,
            fresh.refresh_token.as_deref(),
            fresh.expires_at,
            installed,
        )?;
        // Only where Anthropic actually handed a new refresh token over is a
        // failed write unrecoverable. A Renewal that Rotated nothing leaves the
        // stored refresh token exactly as live as it was, so the write is worth
        // trying again rather than worth a Quarantine (ADR 0006 is about the
        // Rotation, not about the renewal).
        store_it(host, store, &rotated, fresh.refresh_token.is_some())?;

        Ok(fresh.access_token)
    })
}

/// Puts the renewed Credential back, and Quarantines the Account when what
/// could not be stored is a Rotation.
///
/// Where Anthropic Rotated, it retired the previous refresh token the moment it
/// handed the new one over, so a Credential that cannot be stored is not a write
/// to try again: the old one is dead and the new one is gone. This is ADR 0006's
/// crash between two writes, arriving as a failed write — and the reason ADR
/// 0006 says Quarantine could not be deferred past v1.
///
/// Where it did not, none of that is true. A Renewal only sometimes Rotates
/// (ADR 0006), and where it did not the stored refresh token is untouched and
/// still buys a token; `profile::store_credential` leaves a store that refused
/// the write holding what it held before. Quarantining there would be Perch
/// saying an Account is unrecoverable on the strength of a failure that cost it
/// nothing but a cached access token — and a locked keychain during one
/// `perch status --group --refresh` would take a whole Group out that way, each
/// with a reason that is not true.
fn store_it(host: &dyn Host, store: &Store, rotated: &str, rotated_away: bool) -> Step<()> {
    profile::store_credential(host, store, rotated).map_err(|error| {
        if rotated_away {
            Outcome::Quarantined {
                why: Quarantine::RotationLost,
                detail: Some(error.to_string()),
            }
        } else {
            Outcome::Failed(format!(
                "Anthropic renewed this Account without Rotating its refresh \
                 token, so no refresh token was retired and this is not a \
                 Quarantine: {error}\n\
                 A store that refused the write is still holding what it held \
                 before, and there this is worth trying again. A store that \
                 took the write and read it back as something else is said \
                 above — that copy was removed rather than left for Claude Code \
                 to find, and there a `perch relogin` is the way back."
            ))
        }
    })
}

const RATE_LIMITED: &str = "Anthropic is rate-limiting Perch, so nothing about \
                            this Account could be read. The cached figure is \
                            what you see.";

/// Refuses to record figures against an Account the token does not belong to.
///
/// The live Credential is whatever is in the Default Profile, and Perch is not
/// the only thing that writes there: someone who runs `claude` and logs in
/// directly leaves Perch's record of who is active behind. Figures cached under
/// the wrong Account would not look wrong — they would look like that Account
/// having spent quota it never spent, which is the evidence a Cycle ranks on.
fn confirm(host: &dyn Host, token: &str, account: &Account) -> Step<()> {
    match anthropic::whose(host, token) {
        Ok(Some(email)) if !registry::same_name(&email, account.email()) => {
            Err(Outcome::Failed(format!(
                "the Credential Perch would ask with belongs to {email} rather \
                 than to {}, so no figure was recorded against it.",
                account.email()
            )))
        }
        Ok(_) => Ok(()),
        // A profile endpoint Perch no longer recognises is no evidence either
        // way, and no reason to stop reading Utilization. ADR 0019 carves out
        // exactly this and nothing wider: *drift in a reply*.
        //
        // An HTTP failure used to arrive here too, and it is the opposite
        // thing. `/api/oauth/profile` returning 503 during an incident while
        // `/api/oauth/usage` keeps answering is nothing about who the token
        // belongs to, and read as permission it cached one Account's figures
        // under another's — the plausible wrong answer ADR 0019 says this
        // design cannot afford, arriving on the day Anthropic has a bad
        // afternoon.
        Err(Refused::Unrecognised(_)) => Ok(()),
        Err(why) => Err(getting_ready_refused(why)),
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

/// A refusal of the Utilization read itself. A throttle is an outcome of its
/// own here, because this is the endpoint the allowance ADR 0015 is about
/// belongs to, and the cache still answers.
fn reading_refused(why: Refused) -> Outcome {
    match why {
        Refused::Throttled => Outcome::Throttled,
        other => Outcome::Failed(other.to_string()),
    }
}

/// A refusal met before the read — deciding whose token this is, or renewing
/// one. A throttle here is not the Utilization allowance being spent, so it is
/// not reported as one: two limits said in the same words would teach people
/// the wrong thing about the one that matters.
fn getting_ready_refused(why: Refused) -> Outcome {
    let said = match why {
        Refused::Throttled => RATE_LIMITED.to_string(),
        other => other.to_string(),
    };
    Outcome::Failed(said)
}

/// The same, for the renewal, where being turned away is terminal: a refresh
/// token Anthropic will not take is one it has retired, revoked, or never
/// issued, and asking again with the same one gets the same answer forever.
fn not_renewed(why: Refused) -> Outcome {
    match why {
        Refused::Rejected => Outcome::Quarantined {
            why: Quarantine::RenewalRejected,
            detail: None,
        },
        other => getting_ready_refused(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(email: &str, outcome: Outcome) -> Attempt {
        Attempt {
            email: email.to_string(),
            named: email.to_string(),
            outcome,
        }
    }

    #[test]
    fn a_figure_that_was_read_needs_no_line_about_it() {
        let report = Report {
            attempts: vec![attempt("someone@example.com", Outcome::Observed)],
            not_kept: None,
        };
        assert!(report.notes().is_empty(), "the age of the figure says it");
        assert_eq!(report.document()["kept"], true);
    }

    #[test]
    fn every_account_that_lost_a_figure_is_named_with_the_reason() {
        let report = Report {
            attempts: vec![
                attempt("someone@example.com", Outcome::Throttled),
                attempt("overflow@example.com", Outcome::Failed("no token".into())),
            ],
            not_kept: None,
        };

        let notes = report.notes();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].starts_with("someone@example.com: "), "{notes:?}");
        assert!(notes[0].contains("cached figure"), "{notes:?}");
        assert_eq!(notes[1], "overflow@example.com: no token");
    }

    #[test]
    fn a_refresh_nobody_asked_for_is_not_reported_as_one_that_went_well() {
        let report = Report::default();
        assert!(!report.asked());
        assert_eq!(report.document(), serde_json::Value::Null);
    }
}
