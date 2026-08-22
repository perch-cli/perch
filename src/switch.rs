//! The Switch: three ordered steps, and never two.
//!
//! A Capture of the outgoing Credential into its own Profile, a write of the
//! incoming one to the Default Profile, and a patch of the Identity to match —
//! in that order, under Claude Code's locks, with a [`Landing`] written to the
//! registry between the first step and the second
//! (ADR a-switch-is-written-down-first). Shared State is not touched: that is
//! the Run path. [`switch_to`] is the way in and the only one, so `perch switch`
//! and the Watcher differ by a [`Reason`] and by nothing else.

use std::path::Path;

use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::{self, Host};
use crate::lock;
use crate::probe::{self, Credential, Installed, Store};
use crate::profile;
use crate::registry::{self, Account, Active, Quarantine, Registry, Scope};

/// What the Capture found — the part of a Switch worth saying out loud, because
/// it is what protects the Account being left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Captured {
    /// The live Credential went back into the outgoing Account's Profile.
    Copied { from: String },
    /// Nothing was live to Capture — Claude Code is logged out.
    NothingLive,
    /// Something was live, and the Identity beside it names somebody other than
    /// the Account Perch believes is active — so it was left where it was rather
    /// than filed under a Profile it does not belong to.
    NotTheirs {
        /// The Account it was about to be written into.
        outgoing: String,
        /// Who the Identity beside the live Credential names instead.
        live: String,
    },
    /// Something was live, the store handed it over, and it is not a Credential
    /// Perch can make sense of — so it was left where it was: bytes nothing
    /// understands are not a Rotation to lose. Only where the store *answered*;
    /// one that would not is a refusal, in [`capture`].
    Unreadable { outgoing: String, why: String },
    /// Perch holds no active Account, so there was nothing to Capture into.
    NoOutgoing,
    /// The live Credential is already byte-for-byte the one this Switch would
    /// write — the trace of a Switch interrupted after the Credential moved and
    /// before it was recorded. No Rotation to save, whether or not the Account
    /// being left is the Account being switched to.
    NothingToSave,
}

/// Why a Switch is being made, and so what else the save recording it carries.
///
/// A scheduled `perch watcher check` is a process with no memory, so what paces
/// the next one reaches the registry in the *same* save as the Switch it paces
/// (ADR a-watcher-knob-is-arithmetic).
#[derive(Debug)]
pub enum Reason {
    /// `perch switch` — somebody asked for this one.
    Asked,
    /// `perch watcher run` — the Watcher moved unasked, in a loop somebody is
    /// watching. What paces the next round is the
    /// [`Recently`](crate::watch::Recently) that process is already holding, so
    /// nothing is written down beyond the Switch itself. An arm of its own so
    /// that a loop has one to pass rather than claiming a Check nothing ran.
    Loop,
    /// `perch watcher check` — the Watcher moved unasked, and the Scope it moved
    /// within records the Check that did it, in the same save.
    /// [`record_the_switch`] is where that is honored.
    Check {
        /// The Scope the Switch was taken within, which is what the record is
        /// kept per.
        scope: Scope,
        /// When it moved.
        at: DateTime<Utc>,
    },
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
/// The same distinction [`NotLanded`] draws: a failure after the Credential was
/// written but before the Identity was patched has still changed which Account
/// the machine is acting as, which a caller has to answer first.
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
    installed: &Installed,
    incoming: &Account,
    outgoing: Option<&Account>,
    reason: Reason,
) -> std::result::Result<Switched, NotSwitched> {
    let landing = perform(host, perch, installed, incoming, outgoing, registry);
    record_the_switch(host, perch, registry, landing, reason)
}

/// The half of a Switch that reaches the registry, with everything that has to
/// reach it in the same save.
///
/// Split out so the ordering can be asserted against a Landing that moved and
/// then failed — a state no [`FakeHost`](crate::host::FakeHost) produces.
fn record_the_switch(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    landing: Landing,
    reason: Reason,
) -> std::result::Result<Switched, NotSwitched> {
    // Asked before the write, because a Switch that moved starts a Cooldown
    // whether or not it finished.
    let moved = landing.moved();

    // Before `record`, so that the save `record` makes carries both. Only where
    // something moved: pacing the next Check on a Switch that changed nothing
    // would be pacing the Watcher on its failures.
    if moved && let Reason::Check { scope, at } = &reason {
        registry.record_check(scope.word(), *at);
    }

    match landing.record(host, perch, registry) {
        Ok(captured) => Ok(Switched { captured, moved }),
        Err(error) => Err(NotSwitched { error, moved }),
    }
}

/// A Switch under way in this process, and the registry record of it.
///
/// Written after the Capture and before the Credential moves, so a Perch that
/// arrives on the gap knows which two Accounts the live Credential could belong
/// to. `record` consumes it: no reading what a Switch found without recording.
struct Landing {
    outcome: Result<Captured>,
    /// The Account this Switch was to. Held rather than borrowed, so a caller
    /// may hand `record` the `&mut Registry` the Account was read out of.
    incoming: String,
    /// The Account it was leaving, for the same reason and for one more: where
    /// nothing moved, this is who is active, and saying so is what takes the
    /// Landing back off the registry.
    leaving: Option<String>,
    incoming_is_live: bool,
    /// Whether the Landing reached the registry. False for every way a Switch
    /// can fail before the Credential was ever going to move, which is every
    /// way that has nothing to take back.
    wrote_it_down: bool,
}

impl Landing {
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

/// Takes a Landing back off the registry, saying who is active instead. Best
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
            "The Switch itself worked: {incoming}'s Credential is the live one. \
             Perch could not record that, so its own view of which Account is \
             active is behind until this is fixed."
        ))
    })
}

/// Everything the three steps need, established under the locks.
///
/// Under them, not before them: the liveness refusal is a statement about a
/// moment, and taking a lock can take seconds. Once the locks are held nothing
/// can change the answer, which is the only condition under which asking helps.
struct Prepared {
    installed: Installed,
    store: Store,
    credential: Credential,
    /// The `oauthAccount` block to write, ready to splice in.
    identity_block: String,
}

/// Makes `incoming` the active Account, Capturing `outgoing` on the way out.
///
/// `registry` is expected to be settled: handed a stale `outgoing`, the Capture
/// files the live Credential under the wrong Account. `perch` is held across the
/// whole of it, and the Landing write is the first thing a stale hold meets.
fn perform(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    installed: &Installed,
    incoming: &Account,
    outgoing: Option<&Account>,
    registry: &mut Registry,
) -> Landing {
    let leaving = outgoing.map(|outgoing| outgoing.email().to_string());

    // Nothing written and nothing moved, so either of these is a Landing that
    // did not land — the same shape, so the one way out is the same way out.
    let failed = |error| Landing {
        outcome: Err(error),
        incoming: incoming.email().to_string(),
        leaving: leaving.clone(),
        incoming_is_live: false,
        wrote_it_down: false,
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

    let store = match registry::the_default_profile(host) {
        Ok(ground) => ground,
        Err(error) => return failed(error),
    };

    let mut incoming_is_live = false;
    let mut wrote_it_down = false;
    let switched: Result<Captured> = lock::under(host, probe::locks_for(&store), |held| {
        // Every step is slow enough to outlast a hold: a keychain that stops to
        // ask stretches `prepare` or `capture` past the ten seconds the
        // config-file lock goes stale in. Built before the first of them.
        let mut holds = lock::Holds::of(held, perch);

        let prepared =
            holds.around(|| prepare(host, incoming, outgoing, installed.clone(), store))?;

        let captured = holds
            .around(|| capture(host, &prepared, incoming, outgoing, registry))
            .map_err(|error| error.with_note(&nothing_happened(outgoing)))?;

        holds
            .around_a_registry_write(|perch| {
                write_it_down(host, perch, registry, &leaving, incoming)
            })
            .map_err(|error| error.with_note(&nothing_happened(outgoing)))?;
        wrote_it_down = true;

        holds
            .around(|| {
                profile::store_credential(host, &prepared.store, prepared.credential.as_str())
            })
            .map_err(|error| error.with_note(&only_captured(&captured, outgoing, incoming)))?;
        incoming_is_live = true;

        holds
            .around(|| patch_identity(host, &prepared))
            .map_err(|error| error.with_note(&live_but_unnamed(&prepared, outgoing, incoming)))?;

        Ok(captured)
    });

    Landing {
        outcome: switched,
        incoming: incoming.email().to_string(),
        leaving,
        incoming_is_live,
        wrote_it_down,
    }
}

/// Writes down that the Credential is about to move, before it moves.
///
/// The in-memory registry is put back where a save fails, so a caller that goes
/// on to write it — `record`, saving a Quarantine — cannot put a Landing on disk
/// that this call established could not be written.
fn write_it_down(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    leaving: &Option<String>,
    incoming: &Account,
) -> Result<()> {
    let before = registry.begin_landing(leaving.clone(), incoming.email());

    if let Err(error) = registry::save(host, perch, registry) {
        registry.abandon_landing(before);
        return Err(error.with_note(
            "Perch does not move the live Credential until it has written down \
             that it is about to, so nothing was moved.",
        ));
    }
    Ok(())
}

/// Makes an Account's Credential the live one without Capturing what it
/// replaces, writing a Landing as a Switch does. The Credential is read back out
/// of the Account's own Profile, so the store a `perch switch` reads tomorrow is
/// the store this proves today; a `make_live` that moved nothing takes its own
/// Landing back.
pub fn make_live(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
    account: &Account,
    whose: &str,
    installed: &Installed,
) -> std::result::Result<(), NotLanded> {
    let nothing_moved = |error| NotLanded {
        error,
        is_live: false,
    };

    // [`perform`]'s guard, on the other door into the same sequence: a Profile
    // two Accounts share holds one Credential, so the one read out of it here
    // need not be this Account's and nothing afterwards could tell.
    refuse_a_shared_profile(account, registry).map_err(nothing_moved)?;

    let store = registry::the_default_profile(host).map_err(nothing_moved)?;
    let leaving = registry.active().whose().map(str::to_string);

    let mut is_live = false;
    let mut wrote_it_down = false;
    let landed = lock::under(host, probe::locks_for(&store), |held| {
        // Under the locks, for the reason [`Prepared`] gives: the caller asked
        // this minutes ago, across a browser round trip. The Default Profile
        // alone — the Account's own Profile is only read here.
        let mut holds = lock::Holds::of(held, perch);

        holds.around(|| refuse_if_live_in(host, &store.config_dir, whose, installed))?;

        let prepared = holds.around(|| prepare(host, account, None, installed.clone(), store))?;

        holds.around_a_registry_write(|perch| {
            write_it_down(host, perch, registry, &leaving, account)
        })?;
        wrote_it_down = true;

        holds.around(|| {
            profile::store_credential(host, &prepared.store, prepared.credential.as_str())
        })?;
        is_live = true;
        holds.around(|| patch_identity(host, &prepared))
    });

    // On every way out, because the two callers write the registry on different
    // subsets of them: `perch remove` records the successor only where the
    // Credential went live, and `perch relogin` writes nothing at all.
    if wrote_it_down {
        let settled_on = match is_live {
            true => Some(account.email().to_string()),
            false => leaving,
        };
        take_the_landing_back(host, perch, registry, settled_on);
    }

    landed.map_err(|error| NotLanded { error, is_live })
}

/// A `make_live` that stopped part way, and what the machine is holding now.
///
/// The same distinction [`NotSwitched`] draws: a failure after the Credential
/// was written but before the Identity was patched has still changed which
/// Account the machine is acting as.
pub struct NotLanded {
    pub error: PerchError,
    /// Whether the Account's Credential is the live one despite the failure.
    pub is_live: bool,
}

/// No Landing is in flight, so the registry a reader is about to ask tells the
/// truth about who is active. A witness (ADR an-ordering-is-a-type), and the
/// negative of a [`Landing`], so nothing is promoted. Two things earn it:
/// [`resolve_a_landing`] settles a Landing it found, and [`nothing_in_flight`]
/// finds there was none to settle.
pub struct Settled(());

/// The same witness, for a reader that has a Landing to *check* rather than one
/// to settle: a `perch watcher run` says what it is about to watch off a
/// registry it has not locked, and a Landing in flight is the state where it has
/// nothing to say yet, because [`Active::whose`] answers with the Account being
/// *left*. `None` is the whole of what it can answer about a Landing.
pub fn nothing_in_flight(registry: &Registry) -> Option<Settled> {
    match registry.active() {
        Active::Landing { .. } => None,
        Active::Nobody | Active::Settled(_) => Some(Settled(())),
    }
}

/// Settles a registry that holds a Landing, so what follows runs against a
/// registry that tells the truth. A step of its own, ahead of everything else a
/// Switch path does, and cheap where there is nothing to settle: one enum arm
/// and no I/O at all, which is every command on every ordinary machine.
pub fn resolve_a_landing(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    registry: &mut Registry,
) -> Result<Settled> {
    let Active::Landing { leaving, arriving } = registry.active().clone() else {
        return Ok(Settled(()));
    };

    let store = registry::the_default_profile(host)?;

    // Under Claude Code's own locks, and the save is inside them too, so the
    // window they close is the whole of read-decide-record. A Rotation Perch
    // could have locked out would otherwise defeat resolution for good.
    lock::under(host, probe::locks_for(&store), |held| {
        let mut holds = lock::Holds::of(held, perch);

        // A store that will not answer says nothing about what it holds, and
        // what it holds is the whole of the evidence. Refused rather than
        // guessed at.
        let live =
            holds
                .around(|| credentials::read(host, &store))
                .map_err(|would_not_answer| {
                    would_not_answer.with_note(&format!(
                        "A Switch to {arriving} was in flight and was not recorded, \
                     and the live Credential is the only thing that says whether \
                     it happened.\n\
                     Nothing was changed. Make that store readable and run this \
                     again."
                    ))
                })?;

        let settled_on = whose_the_live_credential_is(
            host,
            &mut holds,
            registry,
            leaving.as_deref(),
            &arriving,
            live.as_ref().map(|held| held.credential.as_str()),
        )
        .ok_or_else(|| {
            PerchError::Conflict(the_landing_is_unaccounted_for(
                leaving.as_deref(),
                &arriving,
            ))
        })?;

        registry.settle(settled_on);
        holds.around_a_registry_write(|perch| registry::save(host, perch, registry))?;
        Ok(Settled(()))
    })
}

/// Which Account the live Credential belongs to, or `None` where nothing on the
/// machine says. Two layers of `None`: the outer is *nothing says whose*, the
/// inner is *nobody's, because nothing is live*. It takes the holds rather than
/// being wrapped in one `around`, because the fallback spends a keychain prompt
/// per Account and the config-file lock goes stale after ten seconds.
fn whose_the_live_credential_is(
    host: &dyn Host,
    holds: &mut lock::Holds<'_, '_, '_>,
    registry: &Registry,
    leaving: Option<&str>,
    arriving: &str,
    live: Option<&str>,
) -> Option<Option<String>> {
    // Nothing live is nothing a later Capture could destroy, so this reading has
    // nothing at stake: a `claude /logout` mid-Switch is what it looks like.
    let Some(live) = live else {
        return Some(leaving.map(str::to_string));
    };

    let holding = |holds: &mut lock::Holds<'_, '_, '_>, account: &Account| {
        holds
            .around(|| held_by(host, account))
            .is_some_and(|held| *held == live)
    };

    if registry
        .account(arriving)
        .is_some_and(|account| holding(holds, account))
    {
        return Some(Some(arriving.to_string()));
    }
    if let Some(leaving) = leaving
        && registry
            .account(leaving)
            .is_some_and(|account| holding(holds, account))
    {
        return Some(Some(leaving.to_string()));
    }

    // A `for` rather than a `find`, because the predicate renews the holds and
    // so is `FnMut` over them.
    for account in &registry.accounts {
        if registry::same_name(account.email(), arriving)
            || leaving.is_some_and(|leaving| registry::same_name(account.email(), leaving))
        {
            continue;
        }
        if holding(holds, account) {
            return Some(Some(account.email().to_string()));
        }
    }
    None
}

/// The corner that stays undecidable: a Landing in flight, and a live Credential
/// matching nobody's stored copy. Both readings are named, because the remedies
/// differ and the user is the only one who knows which happened. Not a
/// Quarantine: nothing is lost and the live Credential very likely works.
fn the_landing_is_unaccounted_for(leaving: Option<&str>, arriving: &str) -> String {
    let said = format!(
        "A Switch to {arriving} was written down and never recorded, and the live \
         Credential is not the one Perch holds for {arriving}"
    );
    match leaving {
        Some(leaving) => format!(
            "{said}, nor the one it holds for {leaving}, nor any other it holds — \
             so Perch cannot tell whether that Switch moved anything.\n\
             It may be {arriving}'s, Rotated since the Switch finished. It may be \
             {leaving}'s, Rotated since the Switch failed to start. Nothing on \
             the machine tells the two apart, and writing over the wrong one \
             destroys the only good copy — so nothing was changed.\n\
             `perch relogin {arriving}` finishes that Switch and `perch relogin \
             {leaving}` abandons it: either one replaces whatever is live with a \
             fresh login for the Account you meant."
        ),
        None => format!(
            "{said}, nor any other it holds, and Perch was on no Account before it \
             — so nothing on the machine says whose the live Credential is.\n\
             Nothing was changed, because writing over it would destroy the only \
             good copy there is. `perch relogin {arriving}` replaces whatever is \
             live with a fresh login for {arriving}, which is the way through."
        ),
    }
}

/// Whether the machine already says what a Switch to this Account would make it
/// say, in both the places that have to agree. Not the Identity alone: `claude
/// /logout` empties the live store and leaves `.claude.json` naming whoever was
/// there, so an Identity read on its own says a Switch has already landed onto a
/// machine that is logged out.
pub fn already_landed(host: &dyn Host, installed: &Installed, account: &Account) -> Result<bool> {
    let store = registry::the_default_profile(host)?;
    // An Identity Perch cannot understand is a file naming nobody, so nothing
    // has landed, and `perch switch <the active Account>` is the command that
    // rewrites it. Propagated, it would refuse the repair it exists for.
    let named = probe::read_identity(host, &store, installed)
        .ok()
        .flatten()
        .is_some_and(|identity| registry::same_name(&identity.email, account.email()));

    // A live store holding bytes that are not a Credential has landed nowhere,
    // and the Switch this would turn away is the one that writes a good
    // Credential over the bad one. So `false` rather than an error.
    let usable = matches!(probe::read_credential(host, &store, installed), Ok(Some(_)));

    Ok(named && usable)
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
        "{} and {} share one Profile, so they share one Credential — their \
         addresses differ only in characters a Profile directory does not keep \
         apart.\n\
         Perch cannot act as either of them: whichever Credential that Profile \
         holds is the one a client would run as, whatever Perch says it is, and \
         nothing afterwards could tell the two apart. `perch remove` one of \
         them, then `perch add` it again.",
        account.email(),
        sharer.email(),
    )))
}

fn prepare(
    host: &dyn Host,
    incoming: &Account,
    outgoing: Option<&Account>,
    installed: Installed,
    store: Store,
) -> Result<Prepared> {
    // Before anything is written, and only of the Profile written to. The
    // incoming Account's is only ever read from, and reading takes nothing away
    // from the session using it.
    if let Some(outgoing) = outgoing {
        refuse_if_live(host, outgoing, &installed)?;
    }

    // From whichever of the Profile's two Credential Stores holds one: an
    // Account is switchable to as long as its Credential is somewhere Claude
    // Code would have looked.
    let held = credentials::read(host, &incoming.store(host)?)?.ok_or_else(|| {
        PerchError::Quarantined {
            why: Quarantine::NoCredential,
            said: format!(
                "Perch holds no Credential for {}, so it is Quarantined — it \
                 stays listed and named, and nothing switches to it until it has \
                 been logged into again.\n\
                 Nothing was changed. {}",
                incoming.email(),
                registry::how_to_repair(incoming.email()),
            ),
        }
    })?;
    let credential = probe::understand_credential(
        held.credential,
        &format!("the Credential Perch holds for {}", incoming.email()),
        &installed,
    )?;

    Ok(Prepared {
        identity_block: identity_block_for(host, incoming)?,
        installed,
        store,
        credential,
    })
}

/// Step one: the live Credential goes back where it belongs — and "where it
/// belongs" is the careful part, because Perch is not the only thing that writes
/// the Default Profile. The evidence is the machine's own Identity beside the
/// Credential; one that is absent or unreadable is not evidence against and
/// still Captures, because losing a Rotation is what this step prevents.
fn capture(
    host: &dyn Host,
    prepared: &Prepared,
    incoming: &Account,
    outgoing: Option<&Account>,
    registry: &Registry,
) -> Result<Captured> {
    let Some(outgoing) = outgoing else {
        return Ok(Captured::NoOutgoing);
    };

    // Bytes that are not a Credential are not a Rotation, so a live store
    // holding rubbish is declined. One that *would not answer* says nothing
    // about what it holds and is refused, with nothing written.
    let live = match probe::read_credential(host, &prepared.store, &prepared.installed) {
        Ok(live) => live,
        Err(why @ PerchError::ProbeRefused { .. }) => {
            return Ok(Captured::Unreadable {
                outgoing: outgoing.email().to_string(),
                why: why.to_string(),
            });
        }
        Err(would_not_answer) => {
            return Err(would_not_answer.with_note(&format!(
                "The live Credential could not be read, so it could not be \
                 Captured into {}'s Profile — and it may be that Account's own, \
                 newer than the copy Perch holds. Nothing was changed. Make that \
                 store readable and run this again.",
                outgoing.email(),
            )));
        }
    };
    let Some(live) = live else {
        return Ok(Captured::NothingLive);
    };

    // Ahead of the Identity, because a stale Identity is the whole of what an
    // interrupted Switch is: read as ownership it would file the incoming
    // Credential into the outgoing Account's Profile.
    if live.as_str() == prepared.credential.as_str() {
        return Ok(Captured::NothingToSave);
    }

    // The repair for a Switch that stopped between steps two and three, and the
    // check above has taken the case with nothing to do. What is left has two
    // readings and nothing tells them apart, so neither is acted on.
    if registry::same_name(incoming.email(), outgoing.email()) {
        return Err(PerchError::Conflict(
            the_live_credential_is_unaccounted_for(incoming),
        ));
    }

    // Over the whole of Unicode, as every other comparison of two addresses is.
    // `.claude.json` is Claude Code's file and nothing makes it agree with the
    // registry about the case of a letter outside ASCII.
    if let Ok(Some(identity)) = probe::read_identity(host, &prepared.store, &prepared.installed)
        && !registry::same_name(&identity.email, outgoing.email())
    {
        // The Identity is decisive only where something else agrees with it: it
        // is the one piece of evidence here Perch does not write, and it goes
        // stale in a state Perch itself produces.
        return match corroborates(host, registry, outgoing, &identity.email, live.as_str()) {
            Corroboration::NothingAtStake => Ok(Captured::NothingToSave),
            Corroboration::NotOurs => Ok(Captured::NotTheirs {
                outgoing: outgoing.email().to_string(),
                live: identity.email,
            }),
            Corroboration::Unaccounted => Err(PerchError::Conflict(
                the_identity_is_not_corroborated(outgoing, &identity.email),
            )),
        };
    }

    profile::store_credential(host, &outgoing.store(host)?, live.as_str())?;

    Ok(Captured::Copied {
        from: outgoing.email().to_string(),
    })
}

/// Whether an Identity naming somebody other than the outgoing Account is borne
/// out by anything besides itself.
enum Corroboration {
    /// There is no Rotation here to lose: the live Credential is already exactly
    /// what the outgoing Account's Profile holds, so a Capture would copy a file
    /// over itself and skipping it costs nothing whoever the Identity names.
    NothingAtStake,
    /// The live Credential is not the outgoing Account's to save: the address
    /// belongs to a login Perch does not hold, or to an Account Perch holds
    /// whose own stored copy is exactly what is live.
    NotOurs,
    /// The live Credential is a Rotation of something — it matches neither the
    /// outgoing Account's stored copy nor that of the Account the Identity names
    /// — and nothing on the machine says whose.
    Unaccounted,
}

/// Reads the second opinion, in the order that settles it most cheaply.
///
/// A store that will not answer corroborates nothing, which lands on
/// [`Corroboration::Unaccounted`]: "the keychain was locked" is not evidence
/// that a refresh token is safe to write over.
fn corroborates(
    host: &dyn Host,
    registry: &Registry,
    outgoing: &Account,
    named: &str,
    live: &str,
) -> Corroboration {
    // Asked first, and of the outgoing Account rather than the one named: it is
    // the question with something at stake, and where the live Credential is
    // already that Profile's copy there is no Rotation to lose.
    if held_by(host, outgoing).is_some_and(|held| *held == live) {
        return Corroboration::NothingAtStake;
    }
    let Some(account) = registry.account(named) else {
        return Corroboration::NotOurs;
    };
    match held_by(host, account) {
        Some(held) if *held == live => Corroboration::NotOurs,
        _ => Corroboration::Unaccounted,
    }
}

/// What an Account's own Profile holds, where it can be read at all.
fn held_by(host: &dyn Host, account: &Account) -> Option<Zeroizing<String>> {
    let store = account.store(host).ok()?;
    Some(credentials::read(host, &store).ok()??.credential)
}

/// The refusal for a live Credential an Identity names somebody else for, where
/// that somebody else is an Account Perch holds and is not holding this.
fn the_identity_is_not_corroborated(outgoing: &Account, named: &str) -> String {
    let outgoing = outgoing.email();
    format!(
        "The Identity beside the live Credential names {named}, but the live \
         Credential is not the one Perch holds for {named} either — and Perch is \
         on {outgoing}, so it cannot tell whose Rotation this is.\n\
         It may be {outgoing}'s, made after a Switch that could not finish \
         writing the Identity: writing over it would destroy the only good copy, \
         so nothing was changed.\n\
         It may be {named}'s, Rotated since.\n\
         Either way, `perch relogin {outgoing}` is the way through: a Capture \
         files the live Credential under the Account Perch is on, so that is the \
         one whose fresh login replaces what is there."
    )
}

/// What Perch cannot establish, when the repair for an interrupted Switch finds
/// a live Credential that is not the one it holds. Both readings are named,
/// because the remedies differ and `perch relogin` is the way through either
/// way.
fn the_live_credential_is_unaccounted_for(account: &Account) -> String {
    let email = account.email();
    format!(
        "{email} is the Account Perch is on and the Account asked for, so this is \
         the repair for a Switch that stopped before it finished — but the live \
         Credential is not the one Perch holds for {email}, and Perch cannot tell \
         which of two things that is.\n\
         It may be {email}'s own, Rotated since: writing over it would destroy \
         the only good copy, so nothing was changed.\n\
         It may be a login somebody made outside Perch: `perch add` holds that \
         one as an Account of its own before anything replaces it.\n\
         Either way, `perch relogin {email}` finishes the repair — it replaces \
         whatever is live with a fresh login for {email}."
    )
}

/// Step three: `.claude.json` comes to name the Account whose Credential is now
/// live — that key of it, and nothing else of it
/// (ADR everything-but-the-account).
fn patch_identity(host: &dyn Host, prepared: &Prepared) -> Result<()> {
    let file = &prepared.store.identity_file;
    let patched = match host.read_file(file) {
        Ok(contents) => probe::patch_oauth_account(
            &contents,
            &prepared.identity_block,
            file,
            &prepared.installed,
        )?,
        // No file at all is a Claude Code that has never been run here. One
        // holding the Account and nothing else is exactly what it would write
        // for itself, and leaves it displaying the Account it is acting as.
        Err(host::HostError::NotFound { .. }) => {
            probe::fresh_identity_file(&prepared.identity_block)
        }
        Err(err) => return Err(PerchError::file_read(file.clone(), err)),
    };

    host::write_atomically(host, file, &patched)
        .map_err(|err| PerchError::file_write(file.clone(), err))
}

/// The `oauthAccount` block for an Account. Its own Profile holds the block
/// Claude Code wrote at login, which carries fields beyond the Identity Perch
/// records, so that block is preferred verbatim; one is composed only for an
/// Account that has none, such as the login Adoption took over
/// (ADR a-login-perch-does-not-need).
fn identity_block_for(host: &dyn Host, incoming: &Account) -> Result<String> {
    let kept = incoming.store(host)?.identity_file;
    let held = host
        .read_file(&kept)
        .ok()
        .and_then(|contents| probe::oauth_account_block(&contents).map(str::to_string));

    Ok(held.unwrap_or_else(|| incoming.identity.oauth_account_block()))
}

/// The Profile that was asked about is not a Live Profile: nothing is running
/// against the Credential a write would go under.
///
/// A witness on the terms [`Settled`] sets out — the negative of a **Live
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
/// (ADR a-profile-is-live-by-evidence). Public because two callers ask it
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
fn refuse_if_live_in(
    host: &dyn Host,
    config_dir: &Path,
    whose: &str,
    installed: &Installed,
) -> std::result::Result<Idle, NotIdle> {
    let running =
        probe::live_clients(host, config_dir, installed).map_err(NotIdle::SessionsUnreadable)?;
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

fn nothing_happened(outgoing: Option<&Account>) -> String {
    match outgoing {
        Some(outgoing) => format!(
            "Nothing was switched. {} is still the active Account and its live \
             Credential is untouched.",
            outgoing.email()
        ),
        None => "Nothing was switched.".to_string(),
    }
}

fn only_captured(captured: &Captured, outgoing: Option<&Account>, incoming: &Account) -> String {
    let mut note = match (captured, outgoing) {
        (Captured::Copied { from }, _) => format!(
            "{from}'s live Credential was Captured into its own Profile first, \
             so nothing has been lost."
        ),
        (Captured::NotTheirs { outgoing, live }, _) => format!(
            "The live Credential belongs to {live} rather than to {outgoing}, so \
             it was left where it was and {outgoing}'s Profile is untouched."
        ),
        (_, Some(outgoing)) => format!("{}'s Profile is unchanged.", outgoing.email()),
        (_, None) => String::new(),
    };
    if !note.is_empty() {
        note.push(' ');
    }
    note.push_str(&format!(
        "The live Credential was not replaced, so {} was not made active.",
        incoming.email()
    ));
    note
}

fn live_but_unnamed(prepared: &Prepared, outgoing: Option<&Account>, incoming: &Account) -> String {
    let named = match outgoing {
        Some(outgoing) => outgoing.email().to_string(),
        None => "another Account".to_string(),
    };
    format!(
        "{incoming} is active — its Credential is the live one and was recorded \
         as such — but {file} still names {named}, so Claude Code will act as \
         {incoming} while displaying {named}.\n\
         Run `perch switch {incoming}` again to finish the job.",
        incoming = incoming.email(),
        file = prepared.store.identity_file.display(),
    )
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::host::FakeHost;
    use crate::probe::Identity;

    const INCOMING: &str = "incoming@example.com";
    const OUTGOING: &str = "outgoing@example.com";

    fn two_accounts() -> Registry {
        let mut registry = Registry::default();
        for email in [OUTGOING, INCOMING] {
            registry.upsert(Account {
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
    fn landing(outcome: Result<Captured>, incoming_is_live: bool) -> Landing {
        Landing {
            outcome,
            incoming: INCOMING.to_string(),
            leaving: Some(OUTGOING.to_string()),
            incoming_is_live,
            wrote_it_down: true,
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
            let mut perch = registry::lock(&host).expect("the registry lock is free");
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
            let mut perch = registry::lock(&host).expect("the registry lock is free");
            let mut registry = two_accounts();
            let (said, code) = (error.to_string(), error.exit_code());

            let handed_back = landing(Err(error), true)
                .record(&host, &mut perch, &mut registry)
                .expect_err("the Switch failed");

            assert_eq!(handed_back.to_string(), said, "{what}");
            assert_eq!(handed_back.exit_code(), code, "{what}");
        }
    }

    /// Asserted off the file rather than off the registry in hand, because "one
    /// save carries both" is a claim about what reached disk.
    #[test]
    fn a_check_that_moved_and_then_failed_still_records_the_check() {
        let host = FakeHost::new();
        let mut perch = registry::lock(&host).expect("the registry lock is free");
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
            Reason::Check {
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
        let mut perch = registry::lock(&host).expect("the registry lock is free");
        let mut registry = two_accounts();
        registry
            .declare_group("work")
            .expect("the Group is nameable");

        let not_switched = record_the_switch(
            &host,
            &mut perch,
            &mut registry,
            landing(Err(ordinary()), false),
            Reason::Check {
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

    #[test]
    fn a_quarantine_that_cannot_be_written_does_not_replace_the_failure_either() {
        let host = FakeHost::new();
        let path = registry::registry_path(&host).expect("there is a home to write under");
        let host = host.with_a_disk_that_fills_writing(&path);
        let mut perch = registry::lock(&host).expect("the registry lock is free");
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
}
