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

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::anthropic::{self, QuotaWindows, Refused};
use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock;
use crate::probe::{self, Credential, Store};
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
                self.email,
                Refused::Throttled
            )),
            Outcome::Failed(why) => Some(format!("{}: {why}", self.email)),
            Outcome::Quarantined { why, detail } => Some(format!(
                "{} is Quarantined: {}{}. {}",
                self.email,
                why.because(),
                match detail {
                    Some(detail) => format!(" ({detail})"),
                    None => String::new(),
                },
                registry::how_to_repair(&self.email),
            )),
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
    pub fn notes(&self) -> Vec<String> {
        let mut said: Vec<String> = self.attempts.iter().filter_map(Attempt::note).collect();
        if let Some(not_kept) = &self.not_kept {
            said.push(not_kept.clone());
        }
        said
    }

    /// Says them, before whatever figures they explain. Every surface that
    /// renders Utilization has the same thing to say first, so it is said in
    /// one place.
    pub fn write_notes(&self, out: &mut dyn Write) -> Result<()> {
        for note in self.notes() {
            say(out, &note)?;
        }
        Ok(())
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
pub fn refresh(host: &dyn Host, registry: &mut Registry, emails: &[String]) -> Report {
    let mut report = Report::default();
    let mut anything_to_keep = false;

    for email in emails {
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
            email: email.clone(),
            outcome,
        });
    }

    if anything_to_keep && let Err(error) = registry::save(host, registry) {
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

    let version = probe::claude_version(host)?;
    let store = holding(host, registry, account)?;
    let token = usable_token(host, &store, &version)?;
    confirm(host, &token, account)?;
    let read = anthropic::utilization(host, &token);
    read.map_err(reading_refused)
}

/// Which store holds the Credential to ask with.
///
/// For the active Account that is the Default Profile: what is live there is
/// its Credential, and it is ahead of the copy in its own Profile, which only
/// catches up when a Switch away Captures it (ADR 0006). Every other Account is
/// asked about with the Credential in its own Profile.
fn holding(host: &dyn Host, registry: &Registry, account: &Account) -> Result<Store> {
    if registry.active.as_deref() == Some(account.email()) {
        probe::default_store(host)
    } else {
        account.store(host)
    }
}

/// An access token that can still be asked a question, renewing the Credential
/// when the one there is has run out.
fn usable_token(host: &dyn Host, store: &Store, version: &str) -> Step<String> {
    let credential = credential_in(host, store, version)?;
    if credential.usable_at(host.now()) {
        return Ok(credential.access_token);
    }

    // Asked before the locks are taken, so an Account that was never going to
    // be renewed says so without queueing behind anything, and asked again
    // under them, where the answer is the one that counts.
    refuse_if_live(host, store, version)?;
    renew_under_the_lock(host, store, version)
}

/// Refuses to renew a Credential something else is holding (ADR 0005).
///
/// Anthropic retires the old refresh token when it Rotates one, so renewing a
/// Credential a running Claude Code has in memory logs that session out
/// silently, mid-task.
fn refuse_if_live(host: &dyn Host, store: &Store, version: &str) -> Step<()> {
    let running = probe::live_clients(host, &store.config_dir, version)?;
    if running.is_empty() {
        return Ok(());
    }

    let pids: Vec<String> = running.iter().map(u32::to_string).collect();
    Err(Outcome::Failed(format!(
        "its access token has expired and a client is running against that \
         Profile (pid {}), so renewing it would log that session out. The \
         cached figure is what you see.",
        pids.join(", ")
    )))
}

fn credential_in(host: &dyn Host, store: &Store, version: &str) -> Step<Credential> {
    probe::read_credential(host, store, version)?.ok_or_else(|| {
        Outcome::Failed(
            "Perch holds no Credential for it, so there is nothing to ask \
             Anthropic with."
                .to_string(),
        )
    })
}

/// Renews the Credential in a store, and puts the Rotation back where it came
/// from before the new token is used for anything.
///
/// Under Claude Code's own locks, in Claude Code's own order (ADR 0006), with
/// Claude Code's own double-checked re-read: whoever was holding the lock while
/// Perch waited for it may have renewed the very Credential Perch was about to.
fn renew_under_the_lock(host: &dyn Host, store: &Store, version: &str) -> Step<String> {
    lock::under(host, probe::locks_for(store), |held| {
        // Both of the questions asked before the locks were taken, asked again
        // now that nothing can change the answer underneath Perch.
        refuse_if_live(host, store, version)?;
        let credential = credential_in(host, store, version)?;
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
            version,
        )?;
        // Anthropic has already retired the old refresh token by now, so a
        // Credential that cannot be stored is one nothing can recover — which
        // is worth saying plainly rather than leaving as a keychain error.
        store_it(host, store, &rotated)?;

        Ok(fresh.access_token)
    })
}

/// Puts the Rotated Credential back, and Quarantines the Account when it cannot.
///
/// Anthropic retired the previous refresh token the moment it handed this one
/// over, so a Credential that cannot be stored is not a write to try again: the
/// old one is dead and the new one is gone. This is ADR 0006's crash between two
/// writes, arriving as a failed write — and the reason ADR 0006 says Quarantine
/// could not be deferred past v1.
fn store_it(host: &dyn Host, store: &Store, rotated: &str) -> Step<()> {
    profile::store_credential(host, store, rotated).map_err(|error| Outcome::Quarantined {
        why: Quarantine::RotationLost,
        detail: Some(error.to_string()),
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
        Ok(Some(email)) if !email.eq_ignore_ascii_case(account.email()) => {
            Err(Outcome::Failed(format!(
                "the Credential Perch would ask with belongs to {email} rather \
                 than to {}, so no figure was recorded against it.",
                account.email()
            )))
        }
        Ok(_) => Ok(()),
        // A profile endpoint Perch no longer recognises is no evidence either
        // way, and no reason to stop reading Utilization.
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
