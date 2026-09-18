//! The Switch: three ordered steps, and never two.
//!
//! A Capture of the outgoing Credential into its own Profile, a write of the
//! incoming one to the Default Profile, and a patch of the Identity to match —
//! in that order, under Claude Code's locks, with a [`Landing`] written to the
//! Registry between the first step and the second
//! (ADR a-switch-is-written-down-first). Shared State is not touched: that is
//! the Run path. [`switch_to`] is the way in and the only one: `perch switch`
//! and the Watcher differ by a [`Reason`], a Remove's successor and a Repair
//! by a [`Departure`], and by nothing else.

use chrono::{DateTime, Utc};

use crate::config::Scope;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock;
use crate::lock::Asking;
use crate::registry::{self, Account, Active, Registry, Settled};

pub use crate::providers::provider::Captured;

/// Why a Switch is being made, and so what else the save recording it carries.
///
/// A Watcher is a process a supervisor may restart, so what paces the next round
/// reaches the Registry in the *same* save as the Switch it paces
/// (ADR a-watcher-knob-is-arithmetic).
#[derive(Debug)]
pub enum Reason {
    /// `perch switch` — somebody asked for this one.
    Asked,
    /// `perch watcher run` or `perch watcher check` — the Watcher moved unasked,
    /// and the Scope it moved within records it, in the same save.
    /// [`record_the_switch`] is where that is honored. One arm for both, because
    /// a Cooldown a loop kept in memory alone was one its own Service cleared by
    /// restarting it.
    Unasked {
        /// The Scope the Switch was taken within, which is what the record is
        /// kept per.
        scope: Scope,
        /// When it moved.
        at: DateTime<Utc>,
    },
}

/// What happens on the way out of the Account being left.
pub enum Departure<'a> {
    /// A Switch: the outgoing Account's Rotation is Captured before the live
    /// Credential is overwritten. `None` where Perch is on nobody, which is a
    /// Switch with no Capture to lose.
    Capturing(Option<&'a Account>),
    /// No Capture: the live Credential is overwritten where it lies, once
    /// nothing is running against the Default Profile as `whose`. A Remove's
    /// successor and a Repair leave this way — what was live is either being
    /// given up or was never worth saving.
    Overwriting { whose: &'a str },
}

impl Departure<'_> {
    /// The Account a Capture would save into, which only a Switch has.
    fn captured(&self) -> Option<&Account> {
        match self {
            Departure::Capturing(outgoing) => *outgoing,
            Departure::Overwriting { .. } => None,
        }
    }
}

/// A Switch that landed, and was written down.
#[derive(Debug)]
pub struct Switched {
    pub captured: Captured,
    /// Whether the incoming Account's Credential is the live one. Always true
    /// here, and said anyway so that a caller pacing itself asks one question of
    /// both ways out.
    pub moved: bool,
}

/// A Switch that did not land, and what the machine is holding now.
///
/// A failure after the Credential was written but before the Identity was
/// patched has still changed which Account the machine is acting as, which a
/// caller has to answer first — whichever [`Departure`] it left by.
pub struct NotSwitched {
    /// The failure the user reads, and the one the exit code comes from.
    pub error: PerchError,
    /// Whether the incoming Account's Credential is the live one despite the
    /// failure.
    pub moved: bool,
}

/// The failure, for a caller that has nothing to decide off `moved` and only
/// wants to hand it on.
impl From<NotSwitched> for PerchError {
    fn from(not_switched: NotSwitched) -> PerchError {
        not_switched.error
    }
}

/// Makes `incoming` the active Account and writes down what that came to: the
/// whole of a Switch, and the one door onto one.
///
/// `registry` is expected to be settled; [`resolve_a_landing`] is the command's
/// step rather than this call's, because four commands take it and one Switches.
pub fn switch_to(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    incoming: &Account,
    departure: Departure<'_>,
    reason: Reason,
) -> std::result::Result<Switched, NotSwitched> {
    let landing = perform(host, perch, incoming, departure, registry);
    record_the_switch(host, perch, registry, landing, reason)
}

/// The half of a Switch that reaches the Registry, with everything that has to
/// reach it in the same save.
///
/// Split out so the ordering can be asserted against a Landing that moved and
/// then failed — a state no [`FakeHost`](crate::host::FakeHost) produces.
fn record_the_switch(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    landing: Landing<'_>,
    reason: Reason,
) -> std::result::Result<Switched, NotSwitched> {
    // Asked before the write, because a Switch that moved starts a Cooldown
    // whether or not it finished.
    let moved = landing.moved();

    // Before `record`, so that the save `record` makes carries both. Only where
    // something moved: pacing the next round on a Switch that changed nothing
    // would be pacing the Watcher on its failures.
    if moved && let Reason::Unasked { scope, at } = &reason {
        registry.record_switch(scope.word(), *at);
    }

    match landing.record(host, perch, registry) {
        Ok(captured) => Ok(Switched { captured, moved }),
        Err(error) => Err(NotSwitched { error, moved }),
    }
}

/// A Switch under way in this process, and the Registry record of it.
///
/// Written after the Capture and before the Credential moves, so a Perch that
/// arrives on the gap knows which two Accounts the live Credential could belong
/// to. `record` consumes it: no reading what a Switch found without recording.
struct Landing<'a> {
    // The provider's Default lock covers the final durable record too.
    lease: Option<Box<dyn crate::providers::provider::DefaultChange + 'a>>,
    outcome: Result<Captured>,
    /// The Account this Switch was to. Held rather than borrowed, so a caller
    /// may hand `record` the `&mut Registry` the Account was read out of.
    incoming: String,
    /// The Account it was leaving, for the same reason and for one more: where
    /// nothing moved, this is who is active, and saying so is what takes the
    /// Landing back off the Registry.
    leaving: Option<String>,
    incoming_is_live: bool,
    /// Whether the Landing reached the Registry. False for every way a Switch
    /// can fail before the Credential was ever going to move, which is every
    /// way that has nothing to take back.
    wrote_it_down: bool,
}

impl Landing<'_> {
    /// Whether the incoming Account's Credential is the live one — true of a
    /// Switch that finished, and of one that failed after the Credential was
    /// written but before the Identity was patched. Asked before the write
    /// because a Switch that happened starts a Cooldown whether or not it
    /// finished.
    fn moved(&self) -> bool {
        self.incoming_is_live
    }

    /// Writes down what the Switch did, and hands back what it found: a
    /// Quarantine best-effort, since the failure already says the Account must
    /// be logged into again; then which Account is active wherever the
    /// Credential moved, including out of a failure; then the failure itself, so
    /// the exit code stays the one it earned.
    fn record(
        self,
        host: &dyn Host,
        perch: &mut lock::Held<'_>,
        registry: &mut Registry,
    ) -> Result<Captured> {
        let Landing {
            outcome,
            incoming,
            leaving,
            incoming_is_live,
            wrote_it_down,
            lease: _lease,
        } = self;

        if let Err(PerchError::Quarantined { why, .. }) = &outcome
            && registry.quarantine(&incoming, *why)
        {
            let _ = registry::save(host, perch, registry);
        }

        match outcome {
            Ok(captured) => {
                record_active(host, perch, registry, &incoming)?;
                Ok(captured)
            }
            Err(error) if incoming_is_live => {
                match record_active(host, perch, registry, &incoming) {
                    Ok(()) => Err(error),
                    Err(unrecorded) => Err(error.with_note(&unrecorded.to_string())),
                }
            }
            Err(error) => {
                if wrote_it_down {
                    take_the_landing_back(host, perch, registry, leaving);
                }
                Err(error)
            }
        }
    }
}

/// Takes a Landing back off the Registry, saying who is active instead. Best
/// effort, and the one write in a Switch that is: one left behind is settled by
/// the next Switch off the two Credentials it names. What it buys is a `perch
/// status` that announces no Switch in flight beside a failure saying nothing
/// was switched.
fn take_the_landing_back(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    settled_on: Option<String>,
) {
    registry.settle(settled_on);
    let _ = registry::save(host, perch, registry);
}

/// Records which Account is active, and says what it costs when that write
/// fails: the Switch itself worked, so Perch's own record is behind until this
/// is fixed.
fn record_active(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    incoming: &str,
) -> Result<()> {
    registry.settle(Some(incoming.to_string()));
    registry::save(host, perch, registry).map_err(|error| {
        error.with_note(&format!(
            "The Switch worked: {incoming}'s Credential is the live one. Only \
             the record of it could not be written."
        ))
    })
}

/// The caller holds a settled Registry until the Landing is recorded.
fn perform<'a>(
    host: &'a dyn Host,
    perch: &mut lock::Held<'_>,
    incoming: &Account,
    departure: Departure<'_>,
    registry: &mut Registry,
) -> Landing<'a> {
    let outgoing = departure.captured();
    // Who is being left, whether or not they are Captured: an Overwriting
    // departure still leaves somebody, and the Landing has to name them.
    let leaving = match &departure {
        Departure::Capturing(outgoing) => outgoing.map(|outgoing| outgoing.key().to_string()),
        Departure::Overwriting { .. } => registry.active().whose().map(str::to_string),
    };

    // Nothing written and nothing moved, so either of these is a Landing that
    // did not land — the same shape, so the one way out is the same way out.
    let failed = |error| Landing {
        outcome: Err(error),
        incoming: incoming.key().to_string(),
        leaving: leaving.clone(),
        incoming_is_live: false,
        wrote_it_down: false,
        lease: None,
    };

    if let Err(error) = refuse_a_shared_profile(incoming, registry) {
        return failed(error);
    }

    // The outgoing Account too, because the Capture writes into *its* store: a
    // Profile two Accounts share holds one Credential, so filing the live one
    // there takes the other Account's away with nothing left to tell them apart.
    if let Some(outgoing) = outgoing
        && let Err(error) = refuse_a_shared_profile(outgoing, registry)
    {
        return failed(error);
    }

    let mut incoming_is_live = false;
    let mut wrote_it_down = false;
    let mut lease = None;
    let switched = (|| {
        let request = crate::providers::provider::DefaultRequest {
            incoming: incoming.profile(host)?,
            outgoing: outgoing.map(|account| account.profile(host)).transpose()?,
            known: registry
                .accounts
                .iter()
                .filter(|account| account.provider() == incoming.provider())
                .map(|account| account.profile(host))
                .collect::<Result<Vec<_>>>()?,
            overwrite: match &departure {
                Departure::Overwriting { whose } => Some((*whose).to_string()),
                _ => None,
            },
        };
        let mut prepared = incoming
            .provider()
            .adapter()
            .prepare_default(host, perch, request)?;
        let captured = prepared
            .capture(perch)
            .map_err(|error| error.with_note(NOTHING_SWITCHED))?;
        write_it_down(host, perch, registry, &leaving, incoming)
            .map_err(|error| error.with_note(NOTHING_SWITCHED))?;
        wrote_it_down = true;
        lease = Some(prepared);
        match lease
            .as_mut()
            .expect("the Default guard remains held")
            .apply(perch)
        {
            Ok(()) => incoming_is_live = true,
            Err(failure) => {
                incoming_is_live = failure.moved;
                return Err(if failure.moved {
                    failure.error
                } else {
                    failure.error.with_note(NOTHING_SWITCHED)
                });
            }
        }
        Ok(captured)
    })();

    Landing {
        lease,
        outcome: switched,
        incoming: incoming.key().to_string(),
        leaving,
        incoming_is_live,
        wrote_it_down,
    }
}

/// Writes down that the Credential is about to move, before it moves.
///
/// The in-memory Registry is put back where a save fails, so a caller that goes
/// on to write it — `record`, saving a Quarantine — cannot put a Landing on disk
/// that this call established could not be written.
fn write_it_down(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    leaving: &Option<String>,
    incoming: &Account,
) -> Result<()> {
    let before = registry.begin_landing(leaving.clone(), incoming.key());

    if let Err(error) = registry::save(host, perch, registry) {
        registry.abandon_landing(before);
        return Err(error);
    }
    Ok(())
}

/// Makes an Account's Credential the live one without Capturing what it
/// replaces: [`switch_to`] with a [`Departure::Overwriting`] departure, so the
/// one write sequence and the one rollback serve this door too. The Credential
/// is read back out of the Account's own Profile, so the store a `perch switch`
/// reads tomorrow is the store this proves today.
pub fn make_live(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    account: &Account,
    whose: &str,
) -> std::result::Result<(), NotSwitched> {
    switch_to(
        host,
        perch,
        registry,
        account,
        Departure::Overwriting { whose },
        // Not a Cycle, so no Cooldown starts: whoever asked for this named the
        // Account themselves.
        Reason::Asked,
    )
    .map(|_switched| ())
}

/// A Landing settled, or the walk that settles one stopped part way.
///
/// A stop is its own answer (ADR an-invariant-gets-a-door): the walk's other
/// empty answer is *nothing on the machine says whose the live Credential is*,
/// and a stop reported that way refuses a Landing nothing is wrong with.
pub enum Resolved<E> {
    /// Settled, and written down.
    Settled(Settled),
    /// Nothing was read past the stop and nothing was written, so the Landing is
    /// still in flight and whatever next takes this path settles it. Uninhabited
    /// where the ask cannot answer no, which is every command somebody typed.
    Stopped(E),
}

/// Settles a Registry that holds a Landing, so what follows runs against a
/// Registry that tells the truth. A step of its own, ahead of everything else a
/// Switch path does, and cheap where there is nothing to settle: one enum arm
/// and no I/O, which is every command on every ordinary machine. `still_ours`
/// is a Watcher's; every other caller's is [`crate::commands::a_settled_landing`].
pub fn resolve_a_landing<E>(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    still_ours: Asking<'_, E>,
) -> Result<Resolved<E>> {
    let Active::Landing { leaving, arriving } = registry.active().clone() else {
        // Nothing to settle is the commonest way to earn the witness, and the
        // only one that reads no store.
        let Some(settled) = registry::nothing_in_flight(registry) else {
            unreachable!("the arm above is the whole of what a Landing in flight is")
        };
        return Ok(Resolved::Settled(settled));
    };

    // Ahead of Claude Code's locks rather than under them: a Watcher already
    // told to stop takes nothing from the machine and gives nothing back.
    if let Err(lost) = still_ours() {
        return Ok(Resolved::Stopped(lost));
    }

    let profiles = registry
        .accounts
        .iter()
        .filter(|account| account.provider() == registry.selected_provider())
        .map(|account| account.profile(host))
        .collect::<Result<Vec<_>>>()?;
    let mut inspection = registry
        .selected_provider()
        .adapter()
        .inspect_default(host)?;
    let mut stopped = None;
    let observed =
        inspection.resolve(perch, &profiles, leaving.as_deref(), &arriving, &mut || {
            match still_ours() {
                Ok(()) => true,
                Err(lost) => {
                    stopped = Some(lost);
                    false
                }
            }
        })?;
    use crate::providers::provider::DefaultObservation;
    let settled_on = match observed {
        DefaultObservation::Stopped => {
            return Ok(Resolved::Stopped(
                stopped.expect("the observation stops only when control is withdrawn"),
            ));
        }
        DefaultObservation::Unknown => {
            return Err(PerchError::Conflict(the_landing_is_unaccounted_for(
                leaving.as_deref(),
                &arriving,
            )));
        }
        DefaultObservation::Settled(on) => on,
    };
    let settled = registry.settle(settled_on);
    registry::save(host, perch, registry)?;
    Ok(Resolved::Settled(settled))
}

/// The corner that stays undecidable: a Landing in flight, and a live Credential
/// matching nobody's stored copy. Both readings are named, because the remedies
/// differ and the user is the only one who knows which happened. Not a
/// Quarantine: nothing is lost and the live Credential very likely works.
fn the_landing_is_unaccounted_for(leaving: Option<&str>, arriving: &str) -> String {
    let said = format!(
        "A Switch to {arriving} was written down and never recorded, and the live \
         Credential is none Perch holds"
    );
    match leaving {
        Some(leaving) => format!(
            "{said}. It may be {arriving}'s or {leaving}'s, Rotated since.\n\
             `perch relogin {arriving}` finishes that Switch; `perch relogin \
             {leaving}` abandons it."
        ),
        None => format!(
            "{said}.\n\
             `perch relogin {arriving}` replaces it with a fresh login."
        ),
    }
}

/// Whether the machine already says what a Switch to this Account would make it
/// say, in both the places that have to agree. Not the Identity alone: `claude
/// /logout` empties the live store and leaves `.claude.json` naming whoever was
/// there, so an Identity read on its own says a Switch has already landed onto a
/// machine that is logged out.
pub fn already_landed(host: &dyn Host, account: &Account) -> Result<bool> {
    account
        .provider()
        .adapter()
        .default_matches(host, &account.profile(host)?)
}

/// Refuses to act *as* an Account whose Profile is not its alone: two addresses
/// that flatten to one slug share a Credential Store and a Credential
/// (ADR claude-code-chooses-the-store), so acting as either leaves the machine
/// one Account while Claude Code displays the other. `perch remove` does not
/// ask, because it is the way out.
pub fn refuse_a_shared_profile(account: &Account, registry: &Registry) -> Result<()> {
    let Some(sharer) = registry::sharing_a_profile_with(registry, account) else {
        return Ok(());
    };
    Err(PerchError::Conflict(format!(
        "{} and {} share one Profile, so Perch cannot act as either.\n\
         `perch remove` one of them, then `perch add` it again.",
        account.key(),
        sharer.key(),
    )))
}

/// Kept on every step through the live write, which fails whole: a half-done
/// Switch is what the reader cannot see (ADR a-refusal-is-a-promise).
const NOTHING_SWITCHED: &str = "Nothing was switched.";

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::domain::Identity;
    use crate::holdings;
    use crate::host::FakeHost;
    use crate::registry::Quarantine;

    const INCOMING: &str = "incoming@example.com";
    const OUTGOING: &str = "outgoing@example.com";

    fn two_accounts() -> Registry {
        let mut registry = Registry::default();
        for email in [OUTGOING, INCOMING] {
            registry.upsert(Account {
                storage_key: None,
                provider: crate::providers::provider::Id::Claude,
                provider_identity: None,
                identity: Identity {
                    email: email.to_string(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        registry.settle(Some(OUTGOING.to_string()));
        registry
    }

    /// A Landing as `perform` hands one back: written down, because everything
    /// below is about what `record` does with one that was.
    fn landing<'a>(outcome: Result<Captured>, incoming_is_live: bool) -> Landing<'a> {
        Landing {
            outcome,
            incoming: INCOMING.to_string(),
            leaving: Some(OUTGOING.to_string()),
            incoming_is_live,
            wrote_it_down: true,
            lease: None,
        }
    }

    fn quarantined() -> PerchError {
        PerchError::Quarantined {
            why: Quarantine::NoCredential,
            said: "neither store holds a Credential".to_string(),
        }
    }

    /// Not `Quarantined`, and not one either caller turns into an outcome of
    /// its own: the ordinary failure, which is only ever handed back.
    fn ordinary() -> PerchError {
        PerchError::Other("the store would not answer".to_string())
    }

    /// The four states a Landing can be in, asserted against one another rather
    /// than one at a time. The fifth row is the one no `perform` produces: a
    /// Quarantine diagnosed after the Credential was written, which nothing
    /// after `store_credential` raises.
    #[test]
    fn what_a_landing_records_is_the_same_whoever_asks() {
        struct Case {
            what: &'static str,
            outcome: Result<Captured>,
            moved: bool,
            active: &'static str,
            quarantine: Option<Quarantine>,
        }

        let cases = [
            Case {
                what: "a Switch that finished",
                outcome: Ok(Captured::NothingLive),
                moved: true,
                active: INCOMING,
                quarantine: None,
            },
            Case {
                what: "a Switch that failed before the Credential moved",
                outcome: Err(ordinary()),
                moved: false,
                active: OUTGOING,
                quarantine: None,
            },
            Case {
                what: "a Switch that made the Credential live and then failed",
                outcome: Err(ordinary()),
                moved: true,
                active: INCOMING,
                quarantine: None,
            },
            Case {
                what: "a Switch that found the Account unusable for good",
                outcome: Err(quarantined()),
                moved: false,
                active: OUTGOING,
                quarantine: Some(Quarantine::NoCredential),
            },
            Case {
                what: "a Quarantine diagnosed after the Credential went live",
                outcome: Err(quarantined()),
                moved: true,
                active: INCOMING,
                quarantine: Some(Quarantine::NoCredential),
            },
        ];

        for case in cases {
            let host = FakeHost::new();
            let mut perch = holdings::lock(&host).expect("the registry lock is free");
            let mut registry = two_accounts();
            let failed = case.outcome.is_err();

            let recorded =
                landing(case.outcome, case.moved).record(&host, &mut perch, &mut registry);

            assert_eq!(
                recorded.is_err(),
                failed,
                "{}: it hands back what it was given",
                case.what
            );
            assert_eq!(
                *registry.active(),
                Active::Settled(case.active.to_string()),
                "{}: which Account is active is a fact about which Credential is \
                 live, and a Landing is not left behind either way",
                case.what
            );
            assert_eq!(
                registry.account(INCOMING).and_then(|held| held.quarantine),
                case.quarantine,
                "{}: a Quarantine is written wherever it was diagnosed",
                case.what
            );
        }
    }

    #[test]
    fn recording_a_landing_never_replaces_the_failure_that_stopped_it() {
        for (what, error) in [
            ("an ordinary failure", ordinary()),
            ("a Quarantine", quarantined()),
        ] {
            let host = FakeHost::new();
            let mut perch = holdings::lock(&host).expect("the registry lock is free");
            let mut registry = two_accounts();
            let (said, code) = (error.to_string(), error.exit_code());

            let handed_back = landing(Err(error), true)
                .record(&host, &mut perch, &mut registry)
                .expect_err("the Switch failed");

            assert_eq!(handed_back.to_string(), said, "{what}");
            assert_eq!(handed_back.exit_code(), code, "{what}");
        }
    }

    /// Asserted off the file rather than off the Registry in hand, because "one
    /// save carries both" is a claim about what reached disk.
    #[test]
    fn a_check_that_moved_and_then_failed_still_records_the_check() {
        let host = FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let mut registry = two_accounts();
        registry
            .declare_group("work")
            .expect("the Group is nameable");
        let at = Utc.with_ymd_and_hms(2026, 8, 17, 9, 30, 0).unwrap();

        let not_switched = record_the_switch(
            &host,
            &mut perch,
            &mut registry,
            landing(Err(ordinary()), true),
            Reason::Unasked {
                scope: Scope::Group("work".to_string()),
                at,
            },
        )
        .expect_err("the Switch moved the Credential and then failed");

        assert!(
            not_switched.moved,
            "the Credential moved, which is what the caller decides on"
        );

        let saved = registry::load(&host)
            .expect("the registry is readable")
            .expect("the Switch wrote one");
        assert_eq!(
            saved.checked("work").map(|checked| checked.switched_at),
            Some(at),
            "the save that recorded the Switch carries the Check that made it"
        );
        assert_eq!(
            *saved.active(),
            Active::Settled(INCOMING.to_string()),
            "and records who the live Credential belongs to, as it always did"
        );
    }

    #[test]
    fn a_check_that_moved_nothing_records_no_check() {
        let host = FakeHost::new();
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let mut registry = two_accounts();
        registry
            .declare_group("work")
            .expect("the Group is nameable");

        let not_switched = record_the_switch(
            &host,
            &mut perch,
            &mut registry,
            landing(Err(ordinary()), false),
            Reason::Unasked {
                scope: Scope::Group("work".to_string()),
                at: Utc.with_ymd_and_hms(2026, 8, 17, 9, 30, 0).unwrap(),
            },
        )
        .expect_err("the Switch failed before the Credential moved");

        assert!(!not_switched.moved, "nothing moved");
        assert_eq!(
            registry.checked("work"),
            None,
            "a Switch that changed nothing does not pace the next Check"
        );
    }

    /// The guard is on the door rather than on the callers, so it holds for one
    /// that forgets: `perch remove` chose its successor on `cycle::is_a_candidate`
    /// alone and reached here with a sharer, and `perch relogin` asks separately.
    #[test]
    fn making_an_account_live_is_refused_where_its_profile_is_not_its_alone() {
        let host = FakeHost::new().with_env("HOME", "/Users/someone");
        let mut registry = Registry::default();
        // Two addresses that slug to one directory, so they share one Credential
        // Store and the Credential read out of it need not be either one's.
        for email in ["some-one@example.com", "some.one@example.com"] {
            registry.upsert(Account {
                storage_key: None,
                provider: crate::providers::provider::Id::Claude,
                provider_identity: None,
                identity: Identity {
                    email: email.to_string(),
                    account_uuid: None,
                    organization_name: None,
                    organization_uuid: None,
                },
                plan: None,
                disabled: false,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        let sharer = registry
            .account("some-one@example.com")
            .expect("it was just added")
            .clone();
        let mut perch = crate::holdings::lock(&host).expect("nobody holds it");

        let stopped = make_live(
            &host,
            &mut perch,
            &mut registry,
            &sharer,
            "the Default Profile",
        )
        .expect_err("neither of them can be made live");

        assert!(
            stopped.error.to_string().contains("share one Profile"),
            "{}",
            stopped.error
        );
        assert!(
            !stopped.moved,
            "and it is refused before anything is written"
        );
    }

    #[test]
    fn a_quarantine_that_cannot_be_written_does_not_replace_the_failure_either() {
        let host = FakeHost::new();
        let path = holdings::registry_path(&host).expect("there is a home to write under");
        let host = host.with_a_disk_that_fills_writing(&path);
        let mut perch = holdings::lock(&host).expect("the registry lock is free");
        let mut registry = two_accounts();

        let handed_back = landing(Err(quarantined()), false)
            .record(&host, &mut perch, &mut registry)
            .expect_err("the Switch failed");

        assert_eq!(
            handed_back.exit_code(),
            quarantined().exit_code(),
            "the Quarantine is what the user is told about, not the write"
        );
    }
    #[test]
    fn the_provider_default_guard_outlives_the_final_registry_write() {
        struct Guard<'a>(&'a FakeHost);
        impl crate::providers::provider::DefaultChange for Guard<'_> {
            fn capture(&mut self, _: &mut lock::Held<'_>) -> Result<Captured> {
                unreachable!()
            }
            fn apply(
                &mut self,
                _: &mut lock::Held<'_>,
            ) -> std::result::Result<(), crate::providers::provider::DefaultFailure> {
                unreachable!()
            }
        }
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                let saved = registry::load(self.0).unwrap().unwrap();
                assert_eq!(saved.active().whose(), Some(INCOMING));
            }
        }
        let host = FakeHost::new();
        let mut registry = two_accounts();
        let mut held = holdings::lock(&host).unwrap();
        registry::save(&host, &mut held, &mut registry).unwrap();
        let mut pending = landing(Ok(Captured::NoOutgoing), true);
        pending.lease = Some(Box::new(Guard(&host)));
        pending.record(&host, &mut held, &mut registry).unwrap();
    }
}
