//! The Act of a Round: read the candidates, choose, and Switch.
//!
//! What a round *decides* is [`crate::round`]'s, which reaches no further than
//! the registry; what it *does* about the one decision that moves an Account is
//! this module's — the lowest one reaching `live`, `observe`, `cycle` and
//! `switch` at once (ADR code-lives-where-it-reaches). The ordering is the
//! contract, and it is owned here rather than by a caller: liveness before the
//! burst, the burst before the choice, and the watch asked last, one call
//! before the Switch.

use crate::cycle;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::live;
use crate::lock::{self, Lost};
use crate::observe;
use crate::probe;
use crate::registry::Registry;
use crate::round::{self, Watching};
use crate::switch::{self, NotSwitched};
use crate::watch::{self, Cooled, Outcome, Watcher, nothing_was_switched};

/// The watch this process holds, as one question rather than three.
///
/// `renew`, `still_held` and `asked_to_stop` were asked apart at nine points and five
/// reviews found a long step running between two of them. One call asks all three, and
/// the long steps take this rather than a bare renewal (ADR an-invariant-gets-a-door).
pub struct Watch<'a> {
    host: &'a dyn Host,
    held: lock::Held<'a>,
}

impl<'a> Watch<'a> {
    pub fn taken(host: &'a dyn Host, held: lock::Held<'a>) -> Watch<'a> {
        Watch { host, held }
    }

    /// Renews the hold and says whether this Watcher is still the one to act.
    ///
    /// Called for its answer wherever a round is about to spend something or change
    /// something, and called for the renewal everywhere else — the same call, so the
    /// two cannot come apart.
    pub fn goes_on(&mut self) -> std::result::Result<(), Lost> {
        self.held.renew();
        if !self.held.still_held() {
            return Err(Lost::HandedOver);
        }
        if self.host.asked_to_stop() {
            return Err(Lost::Stopped);
        }
        Ok(())
    }

    /// The renewal without the question, for the one place the answer changes
    /// nothing: the Credential has already moved, and there is no step left to
    /// stop before.
    pub fn kept_up(&mut self) {
        self.held.renew();
    }
}

/// Everything an Act is taken with. One door's worth of context in the shape
/// [`crate::round::Reading`] set, with the [`Cooled`] outside it: the witness is
/// the door, not the context.
pub struct Acting<'a, 'h> {
    pub host: &'h dyn Host,
    pub perch: &'a mut lock::Held<'h>,
    pub registry: &'a mut Registry,
    pub watching: &'a Watching,
    pub watcher: Watcher,
    /// What the round already asked the machine and this Act must not ask again.
    pub probed: &'a probe::Installed<'h>,
    pub watching_alone: &'a mut Watch<'h>,
}

/// The Account is full enough to move off, so this is the whole of what the watcher
/// does about it: read the candidates, choose, and Switch.
///
/// Read here rather than kept warm — the only moment their figures are worth
/// anything, and the moment they are cheapest to get.
pub fn run(acting: Acting<'_, '_>, cooled: &Cooled<'_>) -> Result<Outcome> {
    let Acting {
        host,
        perch,
        registry,
        watching,
        watcher,
        probed,
        watching_alone,
    } = acting;
    let scope = watching.scope.clone();
    let outgoing = watching.account.clone();

    // Not reachable: a round reaches this only on a figure it read, and a figure
    // is read only where the probe answered. Handed on rather than asserted,
    // because what runs this is a Service nobody is watching.
    if let Some(why) = probed.absent() {
        return Err(PerchError::Other(why.to_string()));
    }
    let installed = probed;

    // Asked before the candidates are read: the burst spends an hourly allowance that
    // does not refill early, one read per candidate, and a `perch run` held open in
    // another terminal would spend it every round.
    let places = [live::Place::of_the_profile(host, &outgoing)?];
    let idle = match live::ask(host, &places) {
        live::Answer::Idle(idle) => idle,
        live::Answer::NotIdle(not_idle) => {
            return watch::refused_or_raised(not_idle, installed);
        }
    };

    // The set a Refresh cannot move, walked once: what changes under one is the
    // figures, and those come back out of `refreshed` below.
    let candidates = round::Candidates::of(registry, watching, cooled, &idle);

    // The burst, and the longest thing a round does: one read per candidate, each
    // bounded only at thirty seconds, over as many as the Scope holds. It takes the
    // watch rather than a renewal, so it ends where the watch does.
    let read = observe::refresh(
        host,
        perch,
        registry,
        &candidates.addresses(),
        probed,
        observe::Spending::ItsOwn {
            still_ours: &mut || watching_alone.goes_on(),
        },
    );
    // Before the choice rather than after it: a burst that stopped part way leaves
    // every candidate past that point on whatever figure was cached, and choosing on
    // those is the Switch this round is no longer the one to make.
    if let Some(lost) = read.stopped {
        return Ok(nothing_was_switched(lost));
    }

    // What could not be read, carried into the sentence that says where the watcher
    // went: an Account ranked on a figure from an hour ago is the one thing that can
    // make this Switch land somewhere worse than it left.
    let unread = read.notes();

    // The margin, applied to the figures the burst above has just written.
    let set_aside = watch::set_aside(
        &watching.policy,
        &watching.scope,
        &candidates.refreshed(registry),
    );

    let choice = match cycle::choose(
        registry,
        &scope,
        Some(outgoing.email()),
        &set_aside,
        host.now(),
    ) {
        Ok(choice) => choice,
        // Nowhere worth going is an answer rather than a failure, and both ways of
        // getting there are resolved by waiting.
        Err(error @ (PerchError::NoCandidate(_) | PerchError::NothingToDo(_))) => {
            return Ok(Outcome::Nowhere {
                why: also(error.to_string(), &unread),
                looking_again: watcher.asking_again(watch::NOWHERE_INTERVAL_MILLIS),
            });
        }
        Err(error) => return Err(error),
    };

    // The last thing asked before the one irreversible thing a round does. The burst
    // above is bounded by nothing but the network, so it can outlast the watch — and a
    // Switch made after that is the second Watcher deciding beside the first.
    if let Err(lost) = watching_alone.goes_on() {
        return Ok(nothing_was_switched(lost));
    }
    // One instant for both arrangements, and the beginning rather than the end, because
    // that is the one already written down.
    let acted_at = host.now();
    let switched = switch::switch_to(
        host,
        perch,
        registry,
        installed,
        &choice.account,
        switch::Departure::Capturing(Some(&outgoing)),
        switch::Reason::Unasked {
            scope: scope.clone(),
            at: acted_at,
        },
    );
    watching_alone.kept_up();

    match switched {
        // Where it went, and nothing about why it won: nobody is at the terminal to be
        // owed a reason. Named as the person named it.
        Ok(_switched) => Ok(Outcome::Switched {
            to: registry.named_for_the_user(choice.account.email()),
            unread,
        }),
        // The machine is part way through a Switch, so it is answered before the
        // refusals below whatever the failure was.
        Err(NotSwitched { error, moved: true }) => Err(error),
        // Nothing was changed, and each of these clears itself. A locked keychain, a
        // probe that cannot find Claude Code, a Profile that will not be written: none
        // of those do, and they keep the code the failure earned.
        Err(NotSwitched {
            error:
                error @ (PerchError::Quarantined { .. }
                | PerchError::ProfileLive(_)
                | PerchError::Busy(_)),
            ..
        }) => Ok(Outcome::Refused {
            // A lock is the one of the three a scheduler should come straight back
            // for, and the round is read as `refused` either way: it decided on a
            // figure it had read, which is what tells it from a hold.
            contended: matches!(error, PerchError::Busy(_)),
            why: error.to_string(),
            // Every candidate was read to get here.
            after_reading: true,
        }),
        Err(NotSwitched { error, .. }) => Err(error),
    }
}

/// A sentence, with whatever else has to be said on the same line after it.
fn also(said: String, notes: &[String]) -> String {
    match notes.is_empty() {
        true => said,
        false => format!("{said} {}", notes.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::holdings;
    use crate::host::FakeHost;
    use crate::host::fake::THIS_PROCESS;
    use crate::host::prelude::*;
    use crate::registry::WindowUtilization;
    use crate::watch::{Fullest, Policy, Recently};

    const WATCHED: &str = "watched@example.com";
    const SPARE: &str = "spare@example.com";

    /// The Credential of an Account whose access token ran out long ago, so reading
    /// it at all means Renewing it first — which sends a request.
    const SPENT: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spent","refreshToken":"sk-ant-ort01-spent","expiresAt":1}}"#;

    fn host() -> FakeHost {
        FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("USER", "someone")
    }

    fn account(email: &str, used_percent: f64) -> crate::registry::Account {
        crate::cycle::tests::account(
            email,
            vec![WindowUtilization {
                window: "5-hour".to_string(),
                used_percent,
                resets_at: None,
            }],
        )
    }

    /// The watched Account at 90% beside a spare at `spare_percent`, active and
    /// settled, with the figures already cached.
    fn watching_a_pair(spare_percent: f64) -> Registry {
        let mut registry = Registry::default();
        registry.declare_group("work").expect("a usable name");
        registry.upsert(account(WATCHED, 90.0));
        registry.upsert(account(SPARE, spare_percent));
        registry.settle(Some(WATCHED.to_string()));
        registry
    }

    /// A Credential the spare can be asked with — spent, so its Refresh Renews,
    /// fails on the unarranged endpoint, and leaves it a candidate on its cached
    /// figure rather than Quarantining it.
    fn barely_credentialed(host: &FakeHost, registry: &Registry, email: &str) {
        let store = registry
            .account(email)
            .expect("it was just added")
            .store(host)
            .expect("home is known");
        let [primary, _] = crate::credentials::stores_for(host, &store);
        primary.write(host, SPENT).expect("the store takes it");
    }

    fn watching(registry: &Registry) -> Watching {
        let account = registry
            .account(WATCHED)
            .expect("it was just added")
            .clone();
        let scope = registry.scope_of(&account);
        Watching {
            account,
            scope,
            policy: Policy {
                threshold: 80,
                margin: 10,
            },
        }
    }

    /// Drives [`run`] with the watch taken and the registry held, as a round would.
    fn run_the_act(host: &FakeHost, registry: &mut Registry, watcher: Watcher) -> Result<Outcome> {
        run_the_act_probed(
            host,
            registry,
            watcher,
            probe::Installed::unknown("2.1.221"),
        )
    }

    fn run_the_act_probed(
        host: &FakeHost,
        registry: &mut Registry,
        watcher: Watcher,
        probed: probe::Installed,
    ) -> Result<Outcome> {
        let watching = watching(registry);
        let fullest = Fullest::of(&watching.account).expect("a figure was cached");
        let crossed = fullest.crossed(80).expect("90 is over 80");
        let cooled = crossed
            .cooled(&Recently::nothing(), host.now())
            .expect("nothing was switched recently");

        let mut perch = holdings::lock(host).expect("nobody holds the registry");
        let held = lock::take_all(host, vec![holdings::watcher_lock_spec(host).unwrap()])
            .expect("nobody holds the watch");
        let mut watching_alone = Watch::taken(host, held);

        run(
            Acting {
                host,
                perch: &mut perch,
                registry,
                watching: &watching,
                watcher,
                probed: &probed,
                watching_alone: &mut watching_alone,
            },
            &cooled,
        )
    }

    /// Marks `email`'s Profile Live, the way a running client would.
    fn make_live(host: &FakeHost, email: &str) {
        let dir = holdings::profile_dir_for(host, email).expect("home is known");
        host.set_file(
            probe::session_marker_at(&dir, THIS_PROCESS),
            &probe::session_marker(THIS_PROCESS, host.now()),
        );
    }

    /// The one Account the registry says is active, asserted after every path that
    /// must not have moved anybody.
    fn still_on(registry: &Registry, email: &str) -> bool {
        *registry.active() == crate::registry::Active::Settled(email.to_string())
    }

    #[test]
    fn an_act_reached_with_no_claude_code_hands_the_absence_on_rather_than_asserting() {
        let host = host();
        let mut registry = watching_a_pair(5.0);

        let raised = run_the_act_probed(
            &host,
            &mut registry,
            Watcher::Loop,
            probe::Installed::Absent {
                why: "no Claude Code here".to_string(),
            },
        )
        .expect_err("what runs this is a Service nobody is watching");

        assert!(
            raised.to_string().contains("no Claude Code here"),
            "{raised}"
        );
        assert!(still_on(&registry, WATCHED), "and nothing was switched");
    }

    #[test]
    fn a_client_on_the_outgoing_profile_refuses_before_any_candidate_is_read() {
        let host = host();
        let mut registry = watching_a_pair(5.0);
        make_live(&host, WATCHED);

        let outcome = run_the_act(&host, &mut registry, Watcher::Loop)
            .expect("a running client is an outcome, not a raise");

        assert!(
            matches!(
                outcome,
                Outcome::Refused {
                    after_reading: false,
                    contended: false,
                    ..
                }
            ),
            "nothing was spent finding out where it would have gone: {outcome:?}"
        );
        assert!(
            host.sent_to(crate::anthropic::USAGE_URL).is_empty(),
            "the burst never started"
        );
    }

    #[test]
    fn a_burst_that_was_stopped_chooses_nothing() {
        let host = host().with_interrupt_after_requests(0);
        host.listen_for_interrupts();
        let mut registry = watching_a_pair(5.0);

        let outcome = run_the_act(&host, &mut registry, Watcher::Loop)
            .expect("a stop is an outcome, not a raise");

        assert!(
            matches!(outcome, Outcome::Stopped { .. }),
            "a burst that stopped part way leaves figures it must not choose on: \
             {outcome:?}"
        );
        assert!(
            still_on(&registry, WATCHED),
            "and nothing was switched on them"
        );
    }

    #[test]
    fn a_watch_lost_after_the_burst_switches_nothing() {
        let host = host().with_interrupt_after_requests(1);
        host.listen_for_interrupts();
        let mut registry = watching_a_pair(5.0);
        // The spare's Renewal sends the one request the interrupt counts, so the
        // burst itself finishes and the loss lands on the last ask before the Switch.
        barely_credentialed(&host, &registry, SPARE);

        let outcome = run_the_act(&host, &mut registry, Watcher::Loop)
            .expect("a lost watch is an outcome, not a raise");

        assert!(
            matches!(outcome, Outcome::Stopped { .. }),
            "a Switch after the watch is lost is a second Watcher deciding beside \
             the first: {outcome:?}"
        );
        assert!(still_on(&registry, WATCHED), "the Credential never moved");
    }

    #[test]
    fn nowhere_worth_going_carries_what_could_not_be_read() {
        let host = host();
        // The spare's cached figure sits over the ceiling of 70, and its Refresh
        // fails on the unarranged endpoint — so it is set aside on the old figure
        // and the sentence has to say the figure is old.
        let mut registry = watching_a_pair(95.0);
        barely_credentialed(&host, &registry, SPARE);

        let outcome = run_the_act(&host, &mut registry, Watcher::Check)
            .expect("nowhere to go is an outcome, not a raise");

        let Outcome::Nowhere { why, looking_again } = outcome else {
            panic!("every candidate is set aside: {outcome:?}");
        };
        assert!(
            why.contains("worth Switching to"),
            "the Margin's sentence is quoted: {why}"
        );
        assert_eq!(
            looking_again, None,
            "a Check exits rather than looking again"
        );
    }

    #[test]
    fn a_switch_turned_away_by_a_held_lock_is_refused_as_contended() {
        let host = host();
        let mut registry = watching_a_pair(5.0);
        barely_credentialed(&host, &registry, SPARE);
        // Somebody else is mid-write on the Default Profile, which is where the
        // Switch would land the Credential.
        let store = holdings::the_default_profile(&host).expect("home is known");
        let _held = store.seized(&host).expect("nobody holds them yet");

        let outcome = run_the_act(&host, &mut registry, Watcher::Loop)
            .expect("a held lock is an outcome, not a raise");

        assert!(
            matches!(
                outcome,
                Outcome::Refused {
                    contended: true,
                    after_reading: true,
                    ..
                }
            ),
            "a scheduler should come straight back for a lock: {outcome:?}"
        );
        assert!(still_on(&registry, WATCHED), "nothing was changed");
    }
}
