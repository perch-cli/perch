//! Claude quota reads and Credential renewal.

use super::profile;
use super::service::{self as anthropic, QuotaWindows, Refused};
use crate::domain::Quarantine;
use crate::lock::{Held, StillOurs};
use crate::observe::{Outcome, Step};
use crate::providers::claude::probe::{self, Credential, Installed, Store};
use crate::providers::provider::{DefaultRelation, ProfileContext, ProfileRef as Account};
use crate::{Host, Result, live, name};
use std::path::PathBuf;
use zeroize::Zeroizing;

pub(super) fn observe(
    host: &dyn Host,
    perch: &mut Held<'_>,
    context: &ProfileContext,
    account: &Account,
    installed: &Installed,
    still_ours: StillOurs<'_>,
) -> Step<QuotaWindows> {
    // An Account already known to be beyond repair is not asked again: nothing would be
    // recorded against it, so the read would spend an allowance that does not refill
    // early to learn what Perch wrote down last time.
    if let Some(why) = account.quarantine {
        return Err(Outcome::Quarantined { why, detail: None });
    }

    // Before the first request, so a machine with nothing to ask spends nothing.
    if let Some(why) = installed.absent() {
        return Err(Outcome::Failed {
            why: why.to_string(),
            spent: false,
        });
    }
    let turn = Turn {
        host,
        installed,
        account,
        asked: holding(host, context, account)?,
    };

    let asking = turn
        .usable_token(perch, still_ours)
        .map_err(|outcome| turn.only_off_their_credential(outcome, Because::ItSaysItRanOut))?;
    match turn.read_off(perch, &asking.token, still_ours) {
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
    turn.refuse_if_live(Because::AnthropicRefusedIt)
        .map_err(|outcome| turn.only_off_their_credential(outcome, Because::AnthropicRefusedIt))?;
    let renewed = turn
        .renew_under_the_lock(perch, Because::AnthropicRefusedIt, still_ours)
        .map_err(|outcome| turn.only_off_their_credential(outcome, Because::AnthropicRefusedIt))?;
    turn.read_off(perch, &renewed.token, still_ours)
        .map_err(Turned::settled)
}

/// One Account's turn, as the context every step of it shares: built once where
/// the turn begins, so a step's signature carries only what varies across it.
/// The shape [`crate::act::Acting`] set — one door's worth of context.
struct Turn<'a> {
    host: &'a dyn Host,
    installed: &'a Installed<'a>,
    account: &'a Account,
    asked: Asked,
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
                      it could be read."
                    .to_string(),
                spent: true,
            },
            Turned::Settled(outcome) => outcome,
        }
    }
}

impl Turn<'_> {
    /// Whose the token is, and then what it says about the Account — the pair of
    /// questions one reading asks, off one access token.
    fn read_off(
        &self,
        perch: &mut Held<'_>,
        token: &str,
        still_ours: StillOurs<'_>,
    ) -> std::result::Result<QuotaWindows, Turned> {
        self.confirm(token, still_ours)?;
        // Between the two requests, because they are two: an endpoint that accepts a
        // connection and then says nothing costs thirty seconds each.
        perch.renew();
        match anthropic::utilization(self.host, token, still_ours) {
            Ok(windows) => Ok(windows),
            Err(Refused::Rejected) => Err(Turned::Away),
            Err(why) => Err(Turned::Settled(reading_refused(why))),
        }
    }

    /// Keeps a Quarantine from being recorded off a Credential never established to be
    /// this Account's, answering with a `Failed` that spends what its `Because` says.
    ///
    /// A Quarantine is a terminal recording, so it is owed what a figure is owed
    /// (ADR a-figure-names-its-account). The evidence for it is local.
    fn only_off_their_credential(&self, outcome: Outcome, because: Because) -> Outcome {
        let Outcome::Quarantined { why, detail } = &outcome else {
            return outcome;
        };
        // What the failure underneath said, where it said anything. Both sentences
        // below carry it in the same place, so they read it from the same line.
        let how = match detail {
            Some(detail) => format!(" ({detail})"),
            None => String::new(),
        };

        // A Switch written down and never recorded: a Claude Code Renewal may have
        // retired the copy this reading asked with, so the refusal is evidence about a
        // superseded Credential rather than a broken Account.
        if self.asked.arriving_in_a_landing {
            return Outcome::Failed {
                why: format!(
                    "the Credential in this Account's own Profile could not be used: \
                     {}{how}. A Switch onto it is in flight and was never recorded, \
                     so the working copy may be the live one, and `perch switch {}` \
                     settles which.",
                    why.because(),
                    self.account.key(),
                ),
                spent: because.spent() || why.reached_provider(),
            };
        }

        if self.theirs_by_what_is_here() {
            return outcome;
        }

        Outcome::Failed {
            why: format!(
                "the live Credential could not be used: {}{how}. {} does not name \
                 {}, so it may belong to a login made outside Perch, and nothing was \
                 recorded against this Account. `perch switch {}` puts its own \
                 Credential back in place.",
                why.because(),
                self.asked.store.identity_file.display(),
                self.account.key(),
                self.account.key(),
            ),
            spent: because.spent() || why.reached_provider(),
        }
    }

    /// Whether what this machine holds says the Credential being asked with is this
    /// Account's — the local evidence, for a recording that must not be made off
    /// somebody else's Credential and cannot get an answer out of Anthropic. An
    /// Account's own Profile is a directory only Perch writes into; the Default
    /// Profile is not, so there it is the Identity beside the Credential that says.
    fn theirs_by_what_is_here(&self) -> bool {
        self.asked.its_own_profile || self.named_by_the_identity()
    }

    /// Whether the store's Identity names this Account.
    fn named_by_the_identity(&self) -> bool {
        probe::read_identity(self.host, &self.asked.store, self.installed)
            .ok()
            .flatten()
            .is_some_and(|identity| super::identity::names(&identity, self.account))
    }
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
/// A *settled* Registry rather than [`Registry::is_active`], which answers a Landing
/// with the Account being **left** — off which figures land under the wrong address.
fn holding(host: &dyn Host, context: &ProfileContext, account: &Account) -> Result<Asked> {
    let its_own_profile = account.directory().to_path_buf();
    let shares_its_profile_with = context.shared_with.clone();
    let settled_on_it = context.default == DefaultRelation::Active;
    if settled_on_it {
        let store = crate::providers::claude::layout::default_profile(host)?;
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
        let named_in_a_landing = matches!(
            context.default,
            DefaultRelation::Leaving | DefaultRelation::Arriving
        );
        let mut in_use_from = vec![its_own_profile];
        if named_in_a_landing {
            in_use_from.push(crate::providers::claude::layout::default_profile(host)?.config_dir);
        }
        Ok(Asked {
            in_use_from,
            store: account.store(host)?,
            its_own_profile: true,
            arriving_in_a_landing: context.default == DefaultRelation::Arriving,
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

impl Turn<'_> {
    /// An access token that can still be asked a question, renewing the Credential
    /// when the one there is has run out.
    fn usable_token(&self, perch: &mut Held<'_>, still_ours: StillOurs<'_>) -> Step<Asking> {
        let credential = self.credential_in(Because::ItSaysItRanOut)?;
        if credential.usable_at(self.host.now()) {
            return Ok(Asking {
                token: credential.access_token,
                freshly_renewed: false,
            });
        }

        // Asked before the locks are taken, so an Account that was never going to be
        // renewed says so without queuing, and again under them, where the answer is
        // the one that counts.
        self.refuse_if_live(Because::ItSaysItRanOut)?;
        self.renew_under_the_lock(perch, Because::ItSaysItRanOut, still_ours)
    }

    /// Refuses to renew a Credential something else is holding.
    ///
    /// Anthropic retires the old refresh token when it Rotates one, so renewing a
    /// Credential a running Claude Code holds logs that session out mid-task. Asked
    /// of every directory it could be in use from, and told why: both reasons reach it.
    fn refuse_if_live(&self, because: Because) -> Step<()> {
        // Which directory each client is in, and not only that there is one:
        // `in_use_from` holds two for the active Account, and a refusal naming neither
        // leaves the reader to guess which to quit.
        let places: Vec<live::Place> = self
            .asked
            .in_use_from
            .iter()
            .map(|dir| live::Place::at(crate::providers::provider::Id::Claude, dir))
            .collect();
        let running = match live::ask(self.host, &places) {
            live::Answer::Idle(_) => return Ok(()),
            // Its own `spent`, rather than the `false` a `PerchError` folds to: a
            // doubt met after a request went out is a round that spent one, and the
            // Back-off paces on that.
            live::Answer::NotIdle(live::NotIdle::Unsure(unsure)) => {
                return Err(Outcome::Failed {
                    why: unsure.refusal().to_string(),
                    spent: because.spent(),
                });
            }
            live::Answer::NotIdle(live::NotIdle::Live(clients)) => clients,
        };

        Err(Outcome::Failed {
            why: format!(
                "{} and a client is running against it ({}), so renewing it would \
                 log that session out.",
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
    /// An Account's own Profile holding nothing is terminal, because that Profile is
    /// the only place its Credential lives. The Default Profile holding nothing is a
    /// Claude Code that has been logged out, and the Account's copy is still there.
    fn credential_in(&self, because: Because) -> Step<Credential> {
        probe::read_credential(self.host, &self.asked.store, self.installed)?.ok_or_else(|| {
            if self.asked.its_own_profile {
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

    /// Renews the Credential in the store, and puts the Rotation back before the new
    /// token is used for anything.
    ///
    /// Under Claude Code's own locks, in its order, with its double-checked re-read:
    /// whoever held the lock while Perch waited may have renewed this Credential.
    fn renew_under_the_lock(
        &self,
        perch: &mut Held<'_>,
        because: Because,
        still_ours: StillOurs<'_>,
    ) -> Step<Asking> {
        // A shared Profile is refused here rather than at the two callers, because
        // this is the one door every Renewal goes through. For this Account alone: the
        // others are readable, and their figures are not this one's to lose.
        if let Some(sharer) = &self.asked.shares_its_profile_with {
            return Err(Outcome::Failed {
                why: format!(
                    "{} and it shares one Credential Store with {sharer}, so Renewing \
                     may retire a refresh token that is not this Account's to spend.",
                    because.clause(),
                ),
                spent: because.spent(),
            });
        }

        let host = self.host;
        let store = &self.asked.store;
        store.entered(host, perch, |holds| {
            // Both of the questions asked before the locks were taken, asked again now
            // that nothing can change the answer underneath Perch.
            self.refuse_if_live(because)?;
            // Renewed around the read for the write below's reason: a keychain read is
            // a `security` subprocess, and one that stops to ask for permission takes
            // as long as the answer does.
            let credential = holds.around(|| self.credential_in(because))?;
            if because == Because::ItSaysItRanOut && credential.usable_at(host.now()) {
                // Somebody else renewed it while Perch queued for the lock, so this
                // reading did not: claiming otherwise would report `Turned::Away`
                // about a token Anthropic did not issue here.
                return Ok(Asking {
                    token: credential.access_token,
                    freshly_renewed: false,
                });
            }

            // An access token that has run out and no refresh token to buy another
            // with is the end of what this Credential can do.
            let refresh_token = credential
                .refresh_token
                .clone()
                .ok_or(Outcome::Quarantined {
                    why: Quarantine::NoRefreshToken,
                    detail: None,
                })?;

            // Renewed before the round trip: the config-file lock goes stale in ten
            // seconds and one request can take longer. Both holds, because losing
            // Perch's throws away every figure found.
            let renewal = holds.around(|| anthropic::renew(host, &refresh_token, still_ours));
            let fresh = renewal.map_err(not_renewed)?;

            // Inside `around` for the network call's reason: on macOS this is three
            // subprocesses and a keychain that may stop to ask, so left unrenewed the
            // lock could be taken during the write nothing can undo.
            holds.around(|| {
                let rotated = probe::credential_after_rotation(
                    &credential,
                    &fresh.access_token,
                    fresh.refresh_token.as_ref().map(|token| token.as_str()),
                    fresh.expires_at,
                    self.installed,
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
                 Worth trying again."
                ),
            }
        }
    })
}

const RATE_LIMITED: &str = "Anthropic is rate-limiting Perch, so nothing about \
                            this Account could be read.";

/// What a surface showing figures adds to every read that failed. Added at that
/// surface rather than written into each reason, because a Watcher says the same
/// reasons and uses no cached figure.
impl Turn<'_> {
    /// Refuses to record figures against an Account the token does not belong to.
    ///
    /// Figures cached under the wrong Account would not look wrong: they would look
    /// like that Account having spent quota it never spent, which is the evidence a
    /// Cycle ranks on.
    fn confirm(&self, token: &str, still_ours: StillOurs<'_>) -> std::result::Result<(), Turned> {
        match anthropic::whose(self.host, token, still_ours) {
            Ok(owner) => {
                let matches = match &self.account.provider_identity {
                    Some(expected) => owner.subject.as_ref() == Some(expected),
                    None => name::same_name(&owner.email, self.account.email()),
                };
                if matches {
                    return Ok(());
                }
                Err(Turned::Settled(Outcome::Failed {
                    why: format!(
                        "the Credential Perch would ask with belongs to {} but does not \
                         establish the expected Account identity for {}, so no figure \
                         was recorded against it.",
                        owner.email,
                        self.account.key()
                    ),
                    spent: true,
                }))
            }
            // The one refusal worth telling apart, because a Renewal may answer it: a
            // token Anthropic will not take is the state a Credential that never says
            // when it expires would otherwise stay in for good.
            Err(Refused::Rejected) => Err(Turned::Away),
            // Drift in a reply is no evidence either way, and the carve-out is that
            // and nothing wider: a 503 from `/api/oauth/profile` while the usage
            // endpoint answers would cache one Account's figures under another's.
            Err(Refused::Unrecognized(drift)) => {
                // Said rather than swallowed, because an endpoint that renames a field
                // asks this question of the machine for ever after, and silence makes
                // that indistinguishable from Anthropic answering. `note` says it once.
                self.host.note(&Refused::Unrecognized(drift).to_string());
                if self.account.provider_identity.is_some() {
                    return Err(Turned::Settled(Outcome::Failed {
                        why: "Claude's profile response does not establish this Account's stable subject and Workspace, so no figure was recorded.".into(),
                        spent: true,
                    }));
                }
                if self.theirs_by_what_is_here() {
                    return Ok(());
                }
                Err(Turned::Settled(Outcome::Failed {
                    why: format!(
                        "Anthropic no longer says whose an access token is, and {} \
                         does not name {}, so the live Credential may belong to a \
                         login made outside Perch and no figure was recorded against \
                         it. `perch switch {}` puts this Account's own Credential \
                         back in place.",
                        self.asked.store.identity_file.display(),
                        self.account.key(),
                        self.account.key(),
                    ),
                    spent: true,
                }))
            }
            Err(why) => Err(Turned::Settled(getting_ready_refused(why))),
        }
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
    use crate::host::Refusing;
    use crate::host::prelude::*;

    /// A refusal made before the first request is one a Back-off must not pace,
    /// and one made after a rejection is one it must — for both ways the ask can
    /// come back, not only for the one that names a client.
    #[test]
    fn a_doubt_paces_the_back_off_by_what_the_round_had_already_spent() {
        let dir = std::path::PathBuf::from("/Users/someone/.claude");
        let host = FakeHost::new()
            .with_env("USER", "someone")
            .with_a_path_refusing(
                crate::providers::sessions::sessions_dir(&dir),
                Refusing::List,
                "permission denied",
            );
        host.create_dir_all(&crate::providers::sessions::sessions_dir(&dir))
            .expect("the directory is there and will not be read");
        let asked = Asked {
            store: crate::providers::claude::probe::store_for_profile(&host, &dir)
                .expect("USER is set"),
            its_own_profile: true,
            arriving_in_a_landing: false,
            in_use_from: vec![dir],
            shares_its_profile_with: None,
        };
        let installed = Installed::unknown("2.1.221");
        let account = crate::cycle::tests::account("someone@example.com", vec![])
            .profile(&host)
            .unwrap();
        let turn = Turn {
            host: &host,
            installed: &installed,
            account: &account,
            asked,
        };

        for (because, paced) in [
            (Because::ItSaysItRanOut, false),
            (Because::AnthropicRefusedIt, true),
        ] {
            let refused = turn
                .refuse_if_live(because)
                .expect_err("whether a client is running got no answer");
            assert!(
                matches!(refused, Outcome::Failed { spent, .. } if spent == paced),
                "\"{}\" spends {paced}: {refused:?}",
                because.clause()
            );
        }
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
