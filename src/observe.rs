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
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde_json::json;
use zeroize::Zeroizing;

use crate::anthropic::{self, QuotaWindows, Refused};
use crate::commands::say;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::live;
use crate::lock::{self, Held};
use crate::lock::{Lost, StillOurs};
use crate::name;
use crate::probe::{self, Credential, Installed, Store};
use crate::profile;
use crate::registry::{self, Account, CachedUtilization, Quarantine, Registry};

/// Whose the allowance a refresh spends is (ADR a-watcher-knob-is-arithmetic).
///
/// Named at the call rather than worked out here: the Watcher holds the watch
/// itself, and a lock cannot say whether the caller is its holder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spending {
    /// A Watcher's own round. It is the one pacing this Account, so nothing
    /// stands between it and the read.
    ItsOwn,
    /// A command somebody typed. Where a Watcher is running, the active
    /// Account's figure is already being kept at the Watcher's interval, and a
    /// second reader spends what the Watcher decides with.
    BesideTheWatcher,
}

impl Spending {
    /// Whether this read is one the Watcher has already made: it holds the
    /// watch, this is the Account it Refreshes, and what it last read is younger
    /// than the interval it reads at.
    fn already_read(self, host: &dyn Host, registry: &Registry, email: &str) -> bool {
        if self != Spending::BesideTheWatcher {
            return false;
        }
        if !registry
            .active()
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
        let age = (host.now() - observed.observed_at).num_milliseconds();
        if !(0..crate::watch::REFRESH_INTERVAL_MILLIS as i64).contains(&age) {
            return false;
        }
        // A lock that cannot be asked about is no Watcher: refusing a read over a
        // question Perch could not answer is the worse of the two mistakes.
        registry::watcher_lock_spec(host)
            .ok()
            .and_then(|watch| lock::is_held(host, &watch))
            .unwrap_or(false)
    }
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
    /// Carried rather than derived, because an `Attempt` has no registry and the
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
type Step<T> = std::result::Result<T, Outcome>;

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
            Outcome::Throttled => Some(format!(
                "{}: {}. The cached figure is what you see.",
                self.named,
                Refused::Throttled
            )),
            Outcome::JustRead => Some(format!(
                "{}: a Watcher is reading this Account every {}, and read it \
                 less than that ago. The figure it read is what you see, and \
                 the allowance is left to the Watcher to decide on.",
                self.named,
                crate::watch::how_often(),
            )),
            Outcome::Failed { why, .. } => Some(format!("{}: {why}", self.named)),
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
            Outcome::Throttled => ("throttled", None, Some(Refused::Throttled.to_string())),
            Outcome::Failed { why, .. } => ("failed", None, Some(why.clone())),
            Outcome::Quarantined { why, detail } => {
                ("quarantined", Some(why.as_str()), detail.clone())
            }
            Outcome::Stopped(_) => ("stopped", None, None),
            Outcome::JustRead => ("just_read", None, None),
        };
        json!({
            "email": self.email,
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

    /// Says them, before whatever figures they explain — for a surface that goes on to
    /// show each Account with its Quarantine beside it, which is both of the ones that
    /// write here.
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
///
/// `installed` is asked for rather than probed, because a probe is a `PATH` walk whose
/// answer cannot change under a running process — and carried as the failure it may be,
/// because an Account already Quarantined is answered before it.
pub fn refresh(
    host: &dyn Host,
    perch: &mut Held<'_>,
    registry: &mut Registry,
    emails: &[String],
    installed: &Result<Installed>,
    spending: Spending,
    still_ours: StillOurs<'_>,
) -> Report {
    let mut report = Report::asked_for();
    let mut anything_to_keep = false;

    for email in emails {
        // A round trip to Anthropic each, so the hold is renewed between them and again
        // inside the turn: one Account's turn is up to six requests bounded at thirty
        // seconds, twice the ninety the registry hold goes stale in.
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
        if spending.already_read(host, registry, email) {
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
        let outcome = match observe(
            host,
            perch,
            registry,
            &account,
            installed.as_ref(),
            still_ours,
        ) {
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
    installed: std::result::Result<&Installed, &PerchError>,
    still_ours: StillOurs<'_>,
) -> Step<QuotaWindows> {
    // An Account already known to be beyond repair is not asked again: nothing would be
    // recorded against it, so the read would spend an allowance that does not refill
    // early to learn what Perch wrote down last time.
    if let Some(why) = account.quarantine {
        return Err(Outcome::Quarantined { why, detail: None });
    }

    let installed = installed.map_err(|err| Outcome::Failed {
        why: err.to_string(),
        spent: false,
    })?;
    let asked = &holding(host, registry, account)?;
    // Taking the `Because` it was reached through, because what it answers with
    // is a `Failed`, and every `Failed` says whether the round spent a request.
    let theirs = move |because| {
        move |outcome| {
            only_off_a_credential_that_is_theirs(host, outcome, asked, account, installed, because)
        }
    };

    let asking = usable_token(host, perch, asked, installed, still_ours)
        .map_err(theirs(Because::ItSaysItRanOut))?;
    match read_off(host, perch, &asking.token, account, still_ours) {
        Ok(windows) => return Ok(windows),
        Err(settled @ Turned::Settled(_)) => return Err(settled.settled()),
        Err(Turned::Away) => {}
    }

    // A token this reading has just minted, refused by the server that minted it.
    // Renewing again buys nothing and costs a Rotation, whose failed write is a
    // permanent `RotationLost` — so the contradiction is reported as one.
    if asking.freshly_renewed {
        return Err(Turned::Away.settled());
    }

    // Anthropic would not take the token, and the Credential holding it did not think
    // it had run out — the state one carrying no `expiresAt` is permanently in. Once,
    // and only off a rejection.
    refuse_if_live(host, asked, installed, Because::AnthropicRefusedIt)
        .map_err(theirs(Because::AnthropicRefusedIt))?;
    let renewed = renew_under_the_lock(
        host,
        perch,
        asked,
        installed,
        Because::AnthropicRefusedIt,
        still_ours,
    )
    .map_err(theirs(Because::AnthropicRefusedIt))?;
    read_off(host, perch, &renewed.token, account, still_ours).map_err(Turned::settled)
}

/// What one attempt at a reading came to when it did not come to figures.
enum Turned {
    /// Anthropic would not take the access token. Kept apart from the rest because it
    /// is the one refusal a Renewal might answer.
    Away,
    /// Anything else, already in the form it will be reported in.
    Settled(Outcome),
}

impl Turned {
    /// The outcome to report, for an attempt that will not be tried again.
    fn settled(self) -> Outcome {
        match self {
            // Not a Quarantine: the refresh token bought a renewal, so it is live, and
            // an Account is not unrecoverable because Anthropic contradicted itself
            // inside one command.
            Turned::Away => Outcome::Failed {
                why: "Anthropic renewed this Account's Credential and then would \
                      not accept the token it had just issued, so nothing about \
                      it could be read. The cached figure is what you see."
                    .to_string(),
                spent: true,
            },
            Turned::Settled(outcome) => outcome,
        }
    }
}

/// Whose the token is, and then what it says about the Account — the pair of questions
/// one reading asks, off one access token.
fn read_off(
    host: &dyn Host,
    perch: &mut Held<'_>,
    token: &str,
    account: &Account,
    still_ours: StillOurs<'_>,
) -> std::result::Result<QuotaWindows, Turned> {
    confirm(host, token, account, still_ours)?;
    // Between the two requests, because they are two: an endpoint that accepts a
    // connection and then says nothing costs thirty seconds each.
    perch.renew();
    match anthropic::utilization(host, token, still_ours) {
        Ok(windows) => Ok(windows),
        Err(Refused::Rejected) => Err(Turned::Away),
        Err(why) => Err(Turned::Settled(reading_refused(why))),
    }
}

/// Keeps a Quarantine from being recorded off a Credential never established to be this
/// Account's.
///
/// A Quarantine is a terminal recording, so it is owed what a figure is owed
/// (ADR a-figure-names-its-account). The evidence for it is local.
fn only_off_a_credential_that_is_theirs(
    host: &dyn Host,
    outcome: Outcome,
    asked: &Asked,
    account: &Account,
    installed: &Installed,
    because: Because,
) -> Outcome {
    let Outcome::Quarantined { why, detail } = &outcome else {
        return outcome;
    };
    // What the failure underneath said, where it said anything. Both sentences below
    // carry it in the same place, so they read it from the same line.
    let how = match detail {
        Some(detail) => format!(" ({detail})"),
        None => String::new(),
    };

    // A Switch written down and never recorded: a Claude Code Renewal may have retired
    // the copy this reading asked with, so the refusal is evidence about a superseded
    // Credential rather than a broken Account.
    if asked.arriving_in_a_landing {
        return Outcome::Failed {
            why: format!(
                "the Credential in this Account's own Profile could not be used \
                 — {}{how} — but a Switch onto it is in flight and was never \
                 recorded, so the working copy may be the live one. Nothing was \
                 recorded against this Account. `perch switch {}` settles that \
                 and says which it was.",
                why.because(),
                account.email(),
            ),
            spent: because.spent() || why.reached_anthropic(),
        };
    }

    if asked.its_own_profile || names(host, &asked.store, account, installed) {
        return outcome;
    }

    Outcome::Failed {
        why: format!(
            "the live Credential could not be used — {}{how} — but {} does not \
             name {}, so it may belong to a login made outside Perch and nothing \
             was recorded against this Account. `perch switch {}` puts its own \
             Credential back in place.",
            why.because(),
            asked.store.identity_file.display(),
            account.email(),
            account.email(),
        ),
        spent: because.spent() || why.reached_anthropic(),
    }
}

/// Whether a store's Identity names this Account.
fn names(host: &dyn Host, store: &Store, account: &Account, installed: &Installed) -> bool {
    probe::read_identity(host, store, installed)
        .ok()
        .flatten()
        .is_some_and(|identity| name::same_name(&identity.email, account.email()))
}

/// The store an Account is asked about with, and whose it is.
///
/// Inseparable, because an empty store means opposite things in the two cases: an
/// Account's own Profile holding nothing is unrecoverable, and the Default Profile
/// holding nothing is a Claude Code that is logged out.
struct Asked {
    store: Store,
    /// Whether this is the Account's own Profile rather than the Default one.
    its_own_profile: bool,
    /// Whether a Switch onto this Account is in flight and not yet recorded, so the
    /// copy being asked with may have been overtaken by a Rotation of the live one.
    arriving_in_a_landing: bool,
    /// Every configuration directory a client could be holding this Account's
    /// Credential from, which is what a Renewal has to be refused against.
    ///
    /// More than the store being renewed, because a Rotation retires the refresh token
    /// for an *Account* rather than for a file.
    in_use_from: Vec<PathBuf>,
    /// The other Account whose Profile this one derives too, where there is one.
    ///
    /// The slug flattens everything that is not alphanumeric, so
    /// `user+work@example.com` and `user.work@example.com` share one directory and
    /// therefore one Credential Store. Every path that *acts* asks about it.
    shares_its_profile_with: Option<String>,
}

/// Which store holds the Credential to ask with: the Default Profile for the active
/// Account, and its own Profile for every other.
///
/// A *settled* registry rather than [`Registry::is_active`], which answers a Landing
/// with the Account being **left** — off which figures land under the wrong address.
fn holding(host: &dyn Host, registry: &Registry, account: &Account) -> Result<Asked> {
    let its_own_profile = account.profile_dir(host)?;
    let shares_its_profile_with =
        registry::sharing_a_profile_with(registry, account).map(|held| held.email().to_string());
    let settled_on_it = matches!(
        registry.active(),
        registry::Active::Settled(active) if name::same_name(active, account.email())
    );
    if settled_on_it {
        let store = registry::the_default_profile(host)?;
        // Two directories, and the only case where they differ: the copy being renewed
        // is the live one, and `perch run <this account>` points a client at a Profile
        // whose refresh token the same Rotation would retire.
        Ok(Asked {
            in_use_from: vec![store.config_dir.clone(), its_own_profile],
            store,
            its_own_profile: false,
            arriving_in_a_landing: false,
            shares_its_profile_with,
        })
    } else {
        // A Landing names the two Accounts the live Credential could belong to,
        // so for either of them the Default Profile is a place a client could be
        // holding this Account's from.
        let named_in_a_landing = matches!(registry.active(), registry::Active::Landing { .. })
            && registry.active().names(account.email());
        let mut in_use_from = vec![its_own_profile];
        if named_in_a_landing {
            in_use_from.push(registry::the_default_profile(host)?.config_dir);
        }
        Ok(Asked {
            in_use_from,
            store: account.store(host)?,
            its_own_profile: true,
            arriving_in_a_landing: matches!(
                registry.active(),
                registry::Active::Landing { arriving, .. }
                    if name::same_name(arriving, account.email())
            ),
            shares_its_profile_with,
        })
    }
}

/// How a reading came to want a Renewal: whether the Credential's own account of itself
/// gets a say, what a refusal on the way says happened, and whether the round has spent
/// a request by the time it gets there (ADR an-invariant-gets-a-door).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Because {
    /// The stored Credential says it has run out. One that turns out to be good after
    /// all — renewed by a client while Perch queued for the lock — is left alone,
    /// because Rotating one that had not run out spends the only refresh token there
    /// is.
    ItSaysItRanOut,
    /// Anthropic refused the access token, so the Credential's own account of itself
    /// has been overtaken by evidence: this is the path that reaches one carrying no
    /// `expiresAt`, which claims to be usable for ever.
    AnthropicRefusedIt,
}

impl Because {
    /// The clause a refusal on the way to a Renewal opens with.
    ///
    /// Read off the reason rather than written at each refusal: three of them are
    /// reached from both, and one sentence about an expired token is true at one.
    fn clause(self) -> &'static str {
        match self {
            Because::ItSaysItRanOut => "its access token has expired",
            Because::AnthropicRefusedIt => "Anthropic would not accept its access token",
        }
    }

    /// Whether the round had asked Anthropic something by the time it got here, which
    /// is what the Back-off paces. `AnthropicRefusedIt` *is* a request that went out
    /// and came back, so a refusal after one is a round that spent.
    fn spent(self) -> bool {
        self == Because::AnthropicRefusedIt
    }
}

/// An access token to ask with, and where this reading got it.
struct Asking {
    /// `Zeroizing` for [`probe::Credential`]'s reason: this is a copy of a live access
    /// token, and it would otherwise outlive the reading in freed heap.
    token: Zeroizing<String>,
    /// Whether a Renewal in *this* reading produced it, which decides whether a refusal
    /// from Anthropic is worth renewing over: a token the server minted moments ago and
    /// then refused is a contradiction inside one command rather than a Credential that
    /// has quietly run out.
    freshly_renewed: bool,
}

/// An access token that can still be asked a question, renewing the Credential when the
/// one there is has run out.
fn usable_token(
    host: &dyn Host,
    perch: &mut Held<'_>,
    asked: &Asked,
    installed: &Installed,
    still_ours: StillOurs<'_>,
) -> Step<Asking> {
    let credential = credential_in(host, asked, installed, Because::ItSaysItRanOut)?;
    if credential.usable_at(host.now()) {
        return Ok(Asking {
            token: credential.access_token,
            freshly_renewed: false,
        });
    }

    // Asked before the locks are taken, so an Account that was never going to be
    // renewed says so without queuing, and again under them, where the answer is the
    // one that counts.
    refuse_if_live(host, asked, installed, Because::ItSaysItRanOut)?;
    renew_under_the_lock(
        host,
        perch,
        asked,
        installed,
        Because::ItSaysItRanOut,
        still_ours,
    )
}

/// Refuses to renew a Credential something else is holding.
///
/// Anthropic retires the old refresh token when it Rotates one, so renewing a
/// Credential a running Claude Code holds logs that session out mid-task. Asked of
/// every directory it could be in use from, and told why, since both reasons reach it.
fn refuse_if_live(
    host: &dyn Host,
    asked: &Asked,
    installed: &Installed,
    because: Because,
) -> Step<()> {
    // Which directory each client is in, and not only that there is one: `in_use_from`
    // holds two for the active Account, and a refusal naming neither leaves the reader
    // to guess which to quit.
    let places: Vec<live::Place> = asked.in_use_from.iter().map(live::Place::at).collect();
    let running = match live::ask(host, &places) {
        live::Answer::Idle(_) => return Ok(()),
        // Its own `spent`, rather than the `false` a `PerchError` folds to: a
        // doubt met after a request went out is a round that spent one, and the
        // Back-off paces on that.
        live::Answer::NotIdle(live::NotIdle::Unsure(unsure)) => {
            return Err(Outcome::Failed {
                why: unsure.refusal(installed).to_string(),
                spent: because.spent(),
            });
        }
        live::Answer::NotIdle(live::NotIdle::Live(clients)) => clients,
    };

    Err(Outcome::Failed {
        why: format!(
            "{} and a client is running against it ({}), so renewing it would \
             log that session out. The cached figure is what you see.",
            because.clause(),
            running
                .iter()
                .map(|client| format!("pid {} in {}", client.pid, client.whose))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        spent: because.spent(),
    })
}

/// The Credential to ask with, or what its absence means.
///
/// An Account's own Profile holding nothing is terminal, because that Profile is the
/// only place its Credential lives. The Default Profile holding nothing is a Claude
/// Code that has been logged out, and the Account's copy is still there.
fn credential_in(
    host: &dyn Host,
    asked: &Asked,
    installed: &Installed,
    because: Because,
) -> Step<Credential> {
    probe::read_credential(host, &asked.store, installed)?.ok_or_else(|| {
        if asked.its_own_profile {
            Outcome::Quarantined {
                why: Quarantine::NoCredential,
                detail: None,
            }
        } else {
            Outcome::Failed {
                why: "the Default Profile holds no Credential, so Claude Code is \
                      logged out and there is nothing to ask Anthropic with."
                    .to_string(),
                spent: because.spent(),
            }
        }
    })
}

/// Renews the Credential in a store, and puts the Rotation back before the new token is
/// used for anything.
///
/// Under Claude Code's own locks, in its order, with its double-checked re-read:
/// whoever held the lock while Perch waited may have renewed this Credential.
fn renew_under_the_lock(
    host: &dyn Host,
    perch: &mut Held<'_>,
    asked: &Asked,
    installed: &Installed,
    because: Because,
    still_ours: StillOurs<'_>,
) -> Step<Asking> {
    // A shared Profile is refused here rather than at the two callers, because this is
    // the one door every Renewal goes through. For this Account alone: the others are
    // readable, and their figures are not this one's to lose.
    if let Some(sharer) = &asked.shares_its_profile_with {
        return Err(Outcome::Failed {
            why: format!(
                "{} and it shares one Profile — and so one Credential Store — \
                 with {sharer}, because their addresses differ only in characters \
                 a Profile directory does not keep apart. Renewing may Rotate, \
                 which would retire a refresh token that is not this Account's to \
                 spend. The cached figure is what you see.",
                because.clause(),
            ),
            spent: because.spent(),
        });
    }

    let store = &asked.store;
    lock::under(host, probe::locks_for(store), |held| {
        let mut holds = lock::Holds::of(held, perch);
        // Both of the questions asked before the locks were taken, asked again now that
        // nothing can change the answer underneath Perch.
        refuse_if_live(host, asked, installed, because)?;
        // Renewed around the read for the write below's reason: a keychain read is a
        // `security` subprocess, and one that stops to ask for permission takes as long
        // as the answer does.
        let credential = holds.around(|| credential_in(host, asked, installed, because))?;
        if because == Because::ItSaysItRanOut && credential.usable_at(host.now()) {
            // Somebody else renewed it while Perch queued for the lock, so this reading
            // did not: claiming otherwise would report `Turned::Away` about a token
            // Anthropic did not issue here.
            return Ok(Asking {
                token: credential.access_token,
                freshly_renewed: false,
            });
        }

        // An access token that has run out and no refresh token to buy another with is
        // the end of what this Credential can do.
        let refresh_token = credential
            .refresh_token
            .clone()
            .ok_or(Outcome::Quarantined {
                why: Quarantine::NoRefreshToken,
                detail: None,
            })?;

        // Renewed before the round trip: the config-file lock goes stale in ten seconds
        // and one request can take longer. Both holds, because losing Perch's throws
        // away every figure found.
        let renewal = holds.around(|| anthropic::renew(host, &refresh_token, still_ours));
        let fresh = renewal.map_err(not_renewed)?;

        // Inside `around` for the network call's reason: on macOS this is three
        // subprocesses and a keychain that may stop to ask, so left unrenewed the lock
        // could be taken during the write nothing can undo.
        holds.around(|| {
            let rotated = probe::credential_after_rotation(
                &credential,
                &fresh.access_token,
                fresh.refresh_token.as_ref().map(|token| token.as_str()),
                fresh.expires_at,
                installed,
            )?;
            store_it(
                host,
                store,
                &rotated,
                rotated_away(
                    &refresh_token,
                    fresh.refresh_token.as_ref().map(|token| token.as_str()),
                ),
            )
        })?;

        Ok(Asking {
            token: fresh.access_token,
            freshly_renewed: true,
        })
    })
}

/// Whether the Renewal retired the refresh token that bought it.
///
/// Only a *different* one makes a failed write unrecoverable, and a server is free to
/// hand back what it was given (RFC 6749 §6), so the echo is not one.
fn rotated_away(sent: &str, handed_back: Option<&str>) -> bool {
    handed_back.is_some_and(|fresh| fresh != sent)
}

/// Puts the renewed Credential back, and Quarantines the Account when what could not be
/// stored is a Rotation.
///
/// Where Anthropic Rotated, the old refresh token died the moment the new one arrived,
/// so this is not a write to try again.
fn store_it(host: &dyn Host, store: &Store, rotated: &str, rotated_away: bool) -> Step<()> {
    // A Rotation writes into a Profile the machine already had, so a store
    // that will not answer may be holding the Credential this replaces.
    profile::store_credential(host, store, rotated).map_err(|error| {
        if rotated_away {
            Outcome::Quarantined {
                why: Quarantine::RotationLost,
                detail: Some(error.to_string()),
            }
        } else {
            Outcome::Failed {
                spent: true,
                why: format!(
                    "Anthropic renewed this Account without Rotating its refresh \
                 token, so nothing was retired and this is not a Quarantine: \
                 {error}\n\
                 A store that refused the write still holds what it held \
                 before, so this is worth trying again. One that took the write \
                 and read it back wrong is said above, and there a `perch \
                 relogin` is the way back."
                ),
            }
        }
    })
}

const RATE_LIMITED: &str = "Anthropic is rate-limiting Perch, so nothing about \
                            this Account could be read. The cached figure is \
                            what you see.";

/// Refuses to record figures against an Account the token does not belong to.
///
/// Figures cached under the wrong Account would not look wrong: they would look like
/// that Account having spent quota it never spent, which is the evidence a Cycle ranks
/// on.
fn confirm(
    host: &dyn Host,
    token: &str,
    account: &Account,
    still_ours: StillOurs<'_>,
) -> std::result::Result<(), Turned> {
    match anthropic::whose(host, token, still_ours) {
        Ok(email) if !name::same_name(&email, account.email()) => {
            Err(Turned::Settled(Outcome::Failed {
                why: format!(
                    "the Credential Perch would ask with belongs to {email} \
                     rather than to {}, so no figure was recorded against it.",
                    account.email()
                ),
                spent: true,
            }))
        }
        Ok(_) => Ok(()),
        // The one refusal worth telling apart, because a Renewal may answer it: a token
        // Anthropic will not take is the state a Credential that never says when it
        // expires would otherwise stay in for good.
        Err(Refused::Rejected) => Err(Turned::Away),
        // Drift in a reply is no evidence either way, and the carve-out is that and
        // nothing wider: a 503 from `/api/oauth/profile` while the usage endpoint
        // answers would cache one Account's figures under another's.
        Err(Refused::Unrecognized(drift)) => {
            // Said rather than swallowed, because an endpoint that renames a field
            // turns this check into a no-op for ever and silence makes that
            // indistinguishable from its passing. Once, which is `note`'s job.
            host.note(&Refused::Unrecognized(drift).to_string());
            Ok(())
        }
        Err(why) => Err(Turned::Settled(getting_ready_refused(why))),
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

/// A refusal of the Utilization read itself. A throttle is an outcome of its own here,
/// because this is the endpoint the hourly allowance belongs to and the cache still
/// answers.
fn reading_refused(why: Refused) -> Outcome {
    match why {
        Refused::Throttled => Outcome::Throttled,
        // A request that was never sent is not a reading that failed: reported as
        // one it would pace a Back-off off a question nobody was asked.
        Refused::Stopped(lost) => Outcome::Stopped(lost),
        other => Outcome::Failed {
            why: other.to_string(),
            spent: true,
        },
    }
}

/// A refusal met before the read — deciding whose token this is, or renewing one. A
/// throttle here is not the Utilization allowance being spent, so it is not reported as
/// one: two limits said in the same words would teach people the wrong thing about the
/// one that matters.
fn getting_ready_refused(why: Refused) -> Outcome {
    let said = match why {
        Refused::Throttled => RATE_LIMITED.to_string(),
        Refused::Stopped(lost) => return Outcome::Stopped(lost),
        other => other.to_string(),
    };
    Outcome::Failed {
        why: said,
        spent: true,
    }
}

/// The same, for the renewal, where being turned away is terminal: a refresh token
/// Anthropic will not take is one it has retired, revoked or never issued, and asking
/// again with the same one gets the same answer for ever.
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
    use crate::host::FakeHost;
    use crate::host::prelude::*;

    fn attempt(email: &str, outcome: Outcome) -> Attempt {
        Attempt {
            email: email.to_string(),
            named: email.to_string(),
            outcome,
        }
    }

    /// A refusal made before the first request is one a Back-off must not pace,
    /// and one made after a rejection is one it must — for both ways the ask can
    /// come back, not only for the one that names a client.
    #[test]
    fn a_doubt_paces_the_back_off_by_what_the_round_had_already_spent() {
        let dir = std::path::PathBuf::from("/Users/someone/.claude");
        let host = FakeHost::new()
            .with_env("USER", "someone")
            .with_unlistable_dir(crate::probe::sessions_dir(&dir), "permission denied");
        host.create_dir_all(&crate::probe::sessions_dir(&dir))
            .expect("the directory is there and will not be read");
        let asked = Asked {
            store: crate::probe::store_for_profile(&host, &dir).expect("USER is set"),
            its_own_profile: true,
            arriving_in_a_landing: false,
            in_use_from: vec![dir],
            shares_its_profile_with: None,
        };
        let installed = Installed::unknown("2.1.221");

        for (because, paced) in [
            (Because::ItSaysItRanOut, false),
            (Because::AnthropicRefusedIt, true),
        ] {
            let refused = refuse_if_live(&host, &asked, &installed, because)
                .expect_err("whether a client is running got no answer");
            assert!(
                matches!(refused, Outcome::Failed { spent, .. } if spent == paced),
                "\"{}\" spends {paced}: {refused:?}",
                because.clause()
            );
        }
    }

    /// The Watcher holds the watch across the whole of a burst — one read per
    /// candidate, each bounded only at thirty seconds — while the watch goes stale
    /// in twenty-two and a half minutes. So the hold it hands over is renewed on
    /// the same beat as the registry's, per Account rather than either side.
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
        let mut perch = registry::lock(&host).expect("nobody holds it");
        // Every Account fails to be read, which is beside the point: the renewal
        // happens before the first request either way.
        let installed = Err(PerchError::Other("no Claude Code here".to_string()));

        let mut renewals = 0;
        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &emails,
            &installed,
            Spending::ItsOwn,
            &mut || {
                renewals += 1;
                Ok(())
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
        let [primary, _] = crate::credentials::stores_for(&host, &store);
        primary.write(&host, EXPIRED).expect("the store takes it");

        let mut perch = registry::lock(&host).expect("nobody holds it");
        let installed = Ok(crate::probe::Installed::unknown("2.1.221"));

        let mut asked = 0;
        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &[email.to_string()],
            &installed,
            // The turn is entered, and the stop lands inside it.
            Spending::ItsOwn,
            &mut || {
                asked += 1;
                match asked {
                    1 => Ok(()),
                    _ => Err(Lost::Stopped),
                }
            },
        );

        assert!(
            host.sent_to(crate::anthropic::TOKEN_URL).is_empty(),
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
        let mut perch = registry::lock(&host).expect("nobody holds it");
        let installed = Ok(crate::probe::Installed::unknown("2.1.221"));

        let mut asked = 0;
        let report = refresh(
            &host,
            &mut perch,
            &mut registry,
            &emails,
            &installed,
            Spending::ItsOwn,
            &mut || {
                asked += 1;
                match asked {
                    1 => Ok(()),
                    _ => Err(Lost::Stopped),
                }
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
            let mut perch = registry::lock(&host).expect("nobody holds it");
            let installed = Err(PerchError::Other("no Claude Code here".to_string()));

            let mut asked = 0;
            let report = refresh(
                &host,
                &mut perch,
                &mut registry,
                &emails,
                &installed,
                Spending::ItsOwn,
                &mut || {
                    asked += 1;
                    match asked {
                        1 => Ok(()),
                        _ => Err(stopped_by),
                    }
                },
            );

            assert_eq!(
                report.attempts.len(),
                1,
                "the first Account is read and the burst ends there ({stopped_by:?})"
            );
        }
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
        assert!(notes[0].contains("cached figure"), "{notes:?}");
        assert_eq!(notes[1], "overflow@example.com: no token");
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

    #[test]
    fn a_renewal_that_hands_back_a_different_refresh_token_rotated() {
        assert!(rotated_away(
            "sk-ant-ort01-spent",
            Some("sk-ant-ort01-fresh")
        ));
    }

    #[test]
    fn a_renewal_that_hands_back_nothing_rotated_nothing() {
        assert!(!rotated_away("sk-ant-ort01-spent", None));
    }

    #[test]
    fn a_renewal_that_echoes_the_refresh_token_it_was_given_rotated_nothing() {
        assert!(!rotated_away(
            "sk-ant-ort01-spent",
            Some("sk-ant-ort01-spent")
        ));
    }
}
