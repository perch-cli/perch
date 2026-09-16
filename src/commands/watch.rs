//! `perch watcher run` and `perch watcher check` — the round, taken.
//!
//! What a round *decides* is [`crate::watch`]'s. One policy in three arrangements
//! (ADR the-machine-runs-the-watcher): typed at a terminal, run by the machine's own
//! service manager, or one round for a scheduler — and the difference between them is
//! [`Watcher`] and nothing else.
//!
//! Nothing a round takes is held across the wait — not the Registry lock, not Claude
//! Code's locks — which is what makes a stop safe. The exception is the watcher lock,
//! held for the whole process and renewed as it goes.

use crate::providers::provider::Id;
use std::collections::BTreeMap;
use std::io::Write;

use chrono::{DateTime, Utc};

use crate::act::{self, Acting, Watch};
use crate::adopt;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::{Host, Waited};
use crate::lock;
use crate::observe;
use crate::registry;
use crate::round::{self, Verdict};
use crate::say;
use crate::switch::{self, Resolved};
use crate::trail;
use crate::watch::{self, Pacing, Recently, Voice, Watcher};

/// One round, for whatever scheduled it.
///
/// The same policy and the same decision line as the loop; what differs is that the
/// line goes to standard output for cron to capture and what was decided goes into the
/// exit code (ADR a-watcher-knob-is-arithmetic).
pub fn check(host: &dyn Host, out: &mut dyn Write) -> Result<i32> {
    host.listen_for_interrupts();

    // Fresh, so the one line a Check says is always said in full.
    let mut voice = Voice::quiet();

    // The same lock a loop takes: a Check firing while a Service runs is the double-
    // Switch it exists for. Held rather than refused, and `20` tells a scheduler to
    // come back.
    let watching_alone = match lock::take_all(host, vec![holdings::watcher_lock_spec(host)?]) {
        Ok(held) => held,
        Err(PerchError::Busy(why)) => {
            voice.held(out, &why, None, host.now())?;
            return Ok(crate::error::EXIT_HELD);
        }
        Err(other) => return Err(other),
    };
    let mut watching_alone = Watch::taken(host, watching_alone);

    let providers = watched_providers(host)?;
    let mut codes = Vec::new();
    let mut failures = Vec::new();
    for provider in &providers {
        let mut voice = Voice::quiet();
        let verdict = one_round(
            host,
            Watcher::Check,
            *provider,
            &mut Pacing::none(),
            &mut watching_alone,
        );
        if providers.len() > 1 {
            say::line(out, &format!("{}:", provider.word()))?;
        }
        match verdict {
            Ok(Verdict::Decided(round)) => {
                voice.round(out, &round, host.now())?;
                codes.push(round.outcome.exit_code());
            }
            Ok(Verdict::Lost(lost)) => {
                voice.lost(out, lost, host.now())?;
                return Ok(crate::error::EXIT_HELD);
            }
            Ok(Verdict::NotArranged(why)) | Err(why) => {
                if matches!(why, PerchError::Busy(_)) {
                    voice.held(out, &why.to_string(), None, host.now())?;
                    codes.push(crate::error::EXIT_HELD);
                } else {
                    if providers.len() > 1 {
                        voice.held(out, &why.to_string(), None, host.now())?;
                    }
                    failures.push(why);
                }
            }
        }
    }
    if let Some(error) = failures.into_iter().next() {
        return Err(error);
    }
    Ok(codes
        .into_iter()
        .max()
        .unwrap_or(crate::error::EXIT_NOTHING_TO_DO))
}

/// The loop, for the person who typed it or the Service running it for them.
pub fn keep_watching(host: &dyn Host, out: &mut dyn Write) -> Result<()> {
    // Before anything else, so that a Ctrl-C — or the `SIGTERM` a service manager stops
    // this with — is a request to finish rather than a process killed in the middle of
    // a Switch.
    host.listen_for_interrupts();

    // The two things carried from one round to the next, both in memory and nowhere
    // else: what the loop is waiting out and what it has already said belong to the
    // loop. What paces a Switch does not — it is read off the Registry each round.
    let mut providers: BTreeMap<Id, ProviderWatch> = BTreeMap::new();
    let mut voice = Voice::quiet();

    // Exactly one Watcher per person per machine. Kept by name rather than dropped into
    // a `_`, because a hold nothing renews is one the next Check clears.
    let Some(watching_alone) = take_the_watch(host, out, &mut voice)? else {
        return voice.stopped(out);
    };
    let mut watching_alone = Watch::taken(host, watching_alone);

    say::line(out, &opening(host)?)?;

    loop {
        // Twice a round, and this is the half that bounds the gap: the window is the
        // longest wait plus a round, and a round is bounded by nothing but the network.
        if let Err(lost) = watching_alone.goes_on() {
            return voice.left(out, lost);
        }

        let configured = watched_providers(host)?;
        providers.retain(|id, _| configured.contains(id));
        for id in &configured {
            let state = providers.entry(*id).or_insert_with(ProviderWatch::new);
            if state.due_at > host.now().timestamp_millis() {
                continue;
            }
            let verdict = one_round(
                host,
                Watcher::Loop,
                *id,
                &mut state.pacing,
                &mut watching_alone,
            );
            if configured.len() > 1 {
                say::line(out, &format!("{}:", id.word()))?;
            }
            let waiting_for = match verdict {
                Ok(Verdict::Decided(round)) => {
                    state.voice.round(out, &round, host.now())?;
                    round.waiting_for()
                }
                Ok(Verdict::NotArranged(why)) => {
                    held_before_a_round(out, &mut state.voice, &why.to_string(), host.now())?
                }
                Ok(Verdict::Lost(lost)) => return voice.left(out, lost),
                Err(PerchError::Busy(why)) => {
                    held_before_a_round(out, &mut state.voice, &why, host.now())?
                }
                Err(other) => return Err(other),
            };
            state.due_at = host
                .now()
                .timestamp_millis()
                .saturating_add(waiting_for as i64);
        }
        let now = host.now().timestamp_millis();
        let waiting_for = providers
            .values()
            .map(|state| state.due_at.saturating_sub(now).max(0) as u64)
            .min()
            .unwrap_or(watch::REFRESH_INTERVAL_MILLIS);

        // The other half, here because the round's own work is over: everything above
        // may have waited on Claude Code's locks or on a keychain that stopped to ask.
        if let Err(lost) = watching_alone.goes_on() {
            return voice.left(out, lost);
        }

        // The one place the loop holds nothing it took this round, and therefore How
        // long is the round's to say: read off it rather than worked out again here, so
        // the wait the line promised and the wait taken cannot differ.
        if host.wait(waiting_for) == Waited::Interrupted {
            break;
        }
    }

    voice.stopped(out)
}

/// Becomes the only Watcher on this machine, holding until whoever has the watch gives
/// it back — `None` where it was asked to stop while it was waiting.
///
/// Holding rather than exiting is what lets the watcher lock have a staleness window
/// measured in tens of minutes.
fn take_the_watch<'a>(
    host: &'a dyn Host,
    out: &mut dyn Write,
    voice: &mut Voice,
) -> Result<Option<crate::lock::Held<'a>>> {
    loop {
        match crate::lock::take_all(host, vec![holdings::watcher_lock_spec(host)?]) {
            Ok(held) => return Ok(Some(held)),
            Err(PerchError::Busy(why)) => {
                let waiting_for = held_before_a_round(out, voice, &why, host.now())?;
                if host.wait(waiting_for) == Waited::Interrupted {
                    return Ok(None);
                }
            }
            Err(other) => return Err(other),
        }
    }
}

/// A hold that happened before there was a [`Round`] to hold, and so one with no
/// Account to name; a round's own hold is [`Voice::round`]'s. At the ordinary
/// interval, because nothing here spent a request: a contended Registry, a lock
/// inside its window and a machine nothing arranged are each an answer.
fn held_before_a_round(
    out: &mut dyn Write,
    voice: &mut Voice,
    why: &str,
    now: DateTime<Utc>,
) -> Result<u64> {
    let waiting_for = watch::REFRESH_INTERVAL_MILLIS;
    voice.held(out, why, Some(waiting_for), now)?;
    Ok(waiting_for)
}

/// What the loop is about to start doing, said before it does it.
///
/// The only place the threshold, the interval, the ceiling and the cooldown are said,
/// because no round re-derives them. A machine that is not arranged for watching is not
/// refused here: the loop starts anyway and holds.
fn opening(host: &dyn Host) -> Result<String> {
    // Read rather than insisted on, for the reason the round beneath it holds rather
    // than exits: raising here would end the loop before the first round could hold on
    // it.
    let watching = adopt::ensure_adopted(host).ok().and_then(|registry| {
        // The one reader that asks whether a Landing is in flight rather than settling
        // one: this holds no lock, and a Switch left in flight is exactly the state
        // where there is nothing to say yet.
        let watching = registry::nothing_in_flight(&registry)
            .and_then(|settled| round::permitted(&registry, &settled).ok())?;
        Some((
            registry.named_for_the_user(watching.account.key()),
            watching,
        ))
    });
    let Some((named, watching)) = watching else {
        return Ok(
            "Started. Nothing is being decided yet; the next line says what is \
             holding it. Ctrl-C stops."
                .to_string(),
        );
    };
    Ok(format!(
        "Watching {} {}. Reading how full it is every {}, and Switching within \
         that Scope when its fullest Quota Window reaches {}%, to an Account at \
         {}% or under, and never twice inside {} minutes. Ctrl-C stops.",
        named,
        watching.scope.within(),
        watch::how_often(),
        watching.policy.threshold,
        watching.policy.ceiling(),
        watch::COOLDOWN_MINUTES,
    ))
}

/// One round: read, decide, and act if acting is what was decided.
///
/// The Registry lock is taken here and given back when this returns rather than held
/// for the life of the loop, which would shut every other `perch` out of the machine
/// for as long as the loop ran.
fn one_round<'h>(
    host: &'h dyn Host,
    watcher: Watcher,
    provider: Id,
    pacing: &mut Pacing,
    watching_alone: &mut Watch<'h>,
) -> Result<Verdict> {
    // A machine with no Claude Code login has nothing to adopt. `Busy` is passed
    // through untouched, because both callers answer it differently from "not
    // arranged".
    let (mut perch, mut registry) = match adopt::ensure_adopted_exclusively(host) {
        Ok(both) => both,
        Err(busy @ PerchError::Busy(_)) => return Err(busy),
        Err(not_arranged) => return Ok(Verdict::NotArranged(not_arranged)),
    };

    registry.select_provider(provider);

    // A Switch path, so it resolves a Landing first. Where it refuses, nobody is there
    // to answer, so it travels as the same "not arranged for watching".
    let settled = match switch::resolve_a_landing(host, &mut perch, &mut registry, &mut || {
        watching_alone.goes_on()
    }) {
        Ok(Resolved::Settled(settled)) => settled,
        // Before a figure or a policy has been reached, so there is no Round to report
        // this as and the loop takes it as the stop it is.
        Ok(Resolved::Stopped(lost)) => return Ok(Verdict::Lost(lost)),
        // `Busy` passed through untouched, as at the adoption lock above. It arrives
        // before anything has been read, so there is no figure and nothing was decided,
        // which is what a hold is.
        Err(busy @ PerchError::Busy(_)) => return Err(busy),
        Err(unsettled) => return Ok(Verdict::NotArranged(unsettled)),
    };

    // Handed back rather than raised, and as the failure itself rather than as its
    // sentence, so a Check still exits `18` for an ungrouped Account and `14` for a
    // Scope.
    let watching = match round::permitted(&registry, &settled) {
        Ok(watching) => watching,
        Err(not_arranged) => return Ok(Verdict::NotArranged(not_arranged)),
    };
    let email = watching.account.key().to_string();

    // Read under the lock, so the cooldown a round is held by is the one that was on
    // record when it decided — and read every round rather than carried, because a
    // Watcher this Service restarts would otherwise come back owing nobody a wait.
    let recently = Recently::recorded(registry.checked(watching.scope.word()), host.now());

    // The one Account Refreshed, and nearly all of the network this loop spends.
    // Renewed either side of it, as the loop renews either side of the wait: up to six
    // requests at thirty seconds each go out under this call alone.
    let report = observe::refresh(
        host,
        &mut perch,
        &mut registry,
        std::slice::from_ref(&email),
        observe::Spending::ItsOwn {
            still_ours: &mut || watching_alone.goes_on(),
        },
    );
    // Worth saying and not worth holding a decision over: the figure this round decides
    // on is the one that was just read, and the next round reads its own.
    if let Some(not_kept) = &report.not_kept {
        host.note(not_kept);
    }

    // The Account the Refresh just wrote to, taken out before the closure below needs
    // the Registry back: what a figure is read off is this, and `watching.account` is
    // the copy from before the read.
    let account = registry
        .account(&email)
        .expect("the Account just refreshed is one Perch holds")
        .clone();

    let decided = round::decide(
        round::Reading {
            account: &account,
            report: &report,
            policy: &watching.policy,
            recently: &recently,
            now: host.now(),
        },
        watcher,
        pacing,
        // Reached only through a `Cooled`, which is the whole of what the decision
        // above is for: the one irreversible thing a round does is behind it.
        |cooled, pacing| {
            act::run(
                Acting {
                    host,
                    perch: &mut perch,
                    registry: &mut registry,
                    watching: &watching,
                    watching_alone,
                },
                cooled,
                pacing,
            )
        },
    )?;
    // The one place both a loop and a Check reach, so a round is written down
    // once however it was started.
    if let Some(moved) = decided.outcome.what_it_moved() {
        trail::acted(host, &moved);
    }
    Ok(Verdict::Decided(decided))
}

struct ProviderWatch {
    pacing: Pacing,
    voice: Voice,
    due_at: i64,
}
impl ProviderWatch {
    fn new() -> Self {
        Self {
            pacing: Pacing::none(),
            voice: Voice::quiet(),
            due_at: i64::MIN,
        }
    }
}

fn watched_providers(host: &dyn Host) -> Result<Vec<Id>> {
    let registry = registry::load(host)?.unwrap_or_default();
    let providers: std::collections::BTreeSet<_> = registry
        .accounts
        .iter()
        .map(|account| account.provider())
        .filter(|id| {
            registry
                .provider_settings
                .get(id)
                .is_none_or(|settings| settings.enabled)
        })
        .collect();
    Ok(if providers.is_empty() {
        vec![registry.run_provider]
    } else {
        providers.into_iter().collect()
    })
}
