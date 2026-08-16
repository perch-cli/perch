//! The Switch: three ordered steps, and never two.
//!
//! Making an Account active is a Capture of the outgoing Credential into its
//! own Profile, a write of the incoming one to the Default Profile, and a patch
//! of the Identity to match — in that order, under Claude Code's locks (ADR
//! 0006). The order is not a preference:
//!
//! - Capturing **first** is what stops a Switch poisoning the Account it
//!   leaves. Anthropic retires a refresh token when it Rotates one, so the copy
//!   in an Account's Profile is several Rotations behind by the time you switch
//!   away, and the live copy is the only good one there is.
//! - Patching the Identity **last** means the moment Claude Code disagrees with
//!   itself is as short as possible, and that if anything fails the file still
//!   names an Account whose Credential Perch can put back.
//!
//! Shared State is not touched. No Reconcile, no `projects[<cwd>]`: those are
//! the Run path (ADR 0003, ADR 0010) and a Switch needs none of it, which is
//! exactly why memory, settings, plugins and project history follow the person
//! across Accounts for free.

use std::path::Path;

use crate::credentials;
use crate::error::{PerchError, Result};
use crate::host::{self, Host};
use crate::lock;
use crate::probe::{self, Credential, Installed, Store};
use crate::profile;
use crate::registry::{self, Account, Quarantine, Registry};

/// What the Capture found, which is the part of a Switch worth saying out loud:
/// it is the part that protects the Account being left behind.
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
    /// understands are not a Rotation to lose, and filing them under an
    /// Account's Profile would overwrite the Credential that Account does have
    /// with rubbish.
    ///
    /// Only where the store *answered*. One that would not is not this: it says
    /// nothing about what it holds, so declining there would be skipping the
    /// Capture of a Credential nobody established was bad, and the Switch
    /// writing over it a moment later would be the loss ADR 0006 exists to
    /// prevent. That is a refusal, in [`capture`].
    Unreadable { outgoing: String, why: String },
    /// Perch holds no active Account, so there was nothing to Capture into.
    NoOutgoing,
    /// The live Credential is already byte-for-byte the one this Switch would
    /// write — the repair for a Switch interrupted after it made the incoming
    /// Credential live but before it finished saying so. There is no Rotation to
    /// save and nothing to copy anywhere.
    ///
    /// Whether the Account being left is the Account being switched to makes no
    /// difference: a Switch that stopped before it recorded who was active
    /// leaves Perch naming the *previous* Account as outgoing, and the live
    /// Credential is no more that Account's for it.
    NothingToSave,
}

/// A Switch that has been performed and not yet written down.
///
/// Between [`perform`] returning and [`Landing::record`] there is a machine
/// acting as one Account while Perch's own record names another: the write to
/// the Default Profile is the second of the three steps, and the patch after it
/// can fail. That gap is the whole of what ADR 0006 is about — the next Switch
/// Captures the live Credential into the Profile the registry names, and where
/// that is the wrong Account its only good copy is gone.
///
/// So there is no way to read what a Switch found without recording it first.
/// `record` consumes the Landing and hands back the [`Captured`]; what it
/// writes is not the caller's to sequence, to word or to forget. Two callers
/// used to sequence it themselves, and the second of them dropped a clause the
/// first kept.
///
/// [`Landing::moved`] is the one question with an answer before the write,
/// because a caller pacing itself has to know whether anything happened.
pub struct Landing {
    outcome: Result<Captured>,
    /// The Account this Switch was to. Held rather than borrowed, so a caller
    /// may hand `record` the `&mut Registry` the Account was read out of.
    incoming: String,
    incoming_is_live: bool,
}

impl Landing {
    /// Whether the incoming Account's Credential is the live one — true of a
    /// Switch that finished, and of one that failed after the Credential was
    /// written but before the Identity was patched.
    ///
    /// Asked before the write because a Switch that happened starts a Cooldown
    /// whether or not it finished, and the Cooldown has to reach the registry
    /// in the same save as everything else (ADR 0013). It is a question about
    /// the machine rather than about what the caller must now do, which is what
    /// separates it from everything `record` keeps.
    pub fn moved(&self) -> bool {
        self.incoming_is_live
    }

    /// Writes down what the Switch did, and hands back what it found.
    ///
    /// Three things, in this order, none of them optional:
    ///
    /// - A Quarantine, where the Switch did not merely fail but found the
    ///   incoming Account unusable for good. Best effort, deliberately: the
    ///   failure the user is about to read already says the Account has to be
    ///   logged into again, and losing it over a registry Perch could not write
    ///   would be a poor trade. The worst a missed write costs is making the
    ///   same discovery next time.
    /// - Which Account is active, wherever the Credential moved — including on
    ///   the way out of a failure. Being active is a fact about which
    ///   Credential is in the Default Profile, not a wish, and recording
    ///   anything else sends the next Capture into the wrong Profile.
    /// - The failure that got us here, kept. A write that could not be recorded
    ///   is worth saying and is not worth losing the original over, so the user
    ///   is told both and the exit code stays the one the failure earned.
    ///
    /// Nothing here decides what a Quarantine is: the failure itself says which
    /// one, diagnosed where it was diagnosed, and this reads it off the error.
    pub fn record(
        self,
        host: &dyn Host,
        perch: &mut lock::Held<'_>,
        registry: &mut Registry,
    ) -> Result<Captured> {
        if let Err(PerchError::Quarantined { why, .. }) = &self.outcome
            && registry.quarantine(&self.incoming, *why)
        {
            let _ = registry::save(host, perch, registry);
        }

        match self.outcome {
            Ok(captured) => {
                record_active(host, perch, registry, &self.incoming)?;
                Ok(captured)
            }
            Err(error) if self.incoming_is_live => {
                match record_active(host, perch, registry, &self.incoming) {
                    Ok(()) => Err(error),
                    Err(unrecorded) => Err(error.with_note(&unrecorded.to_string())),
                }
            }
            Err(error) => Err(error),
        }
    }
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
    registry.active = Some(incoming.to_string());
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
/// moment, and taking a lock can take seconds. A `claude` started during that
/// wait is one the refusal never saw, and the Switch would then replace the
/// Credential that session is holding — the mid-task logout ADR 0005 exists to
/// prevent. Once the locks are held, nothing can change the answer, which is
/// the only condition under which asking is worth anything.
struct Prepared {
    installed: Installed,
    store: Store,
    credential: Credential,
    /// The `oauthAccount` block to write, ready to splice in.
    identity_block: String,
}

/// Makes `incoming` the active Account, Capturing `outgoing` on the way out.
///
/// `perch` is the caller's hold on the registry, renewed alongside Claude Code's
/// locks for the same reason and against the same hazard. It is held from before
/// the load to after the save, so every slow step below runs under it — and it
/// goes stale in ninety seconds. A keychain that stopped to ask the user for
/// permission ran that out, another Perch cleared the artifact and worked under
/// it, and the [`Landing::record`] that follows this then refused: the live
/// Credential belongs to `incoming` while the registry still names `outgoing`,
/// so the *next* Switch Captures the live Credential into `outgoing`'s Profile
/// and destroys its only copy (ADR 0006). Re-running the Switch does not repair
/// that, because `already_there` answers before the recording is reached.
///
/// Returns a [`Landing`] rather than a `Result`, because whether a Switch
/// succeeded is not a thing a caller may act on before it has written down what
/// happened.
pub fn perform(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    installed: &Installed,
    incoming: &Account,
    outgoing: Option<&Account>,
    registry: &Registry,
) -> Landing {
    // Nothing has been written and nothing can have moved, so either of these is
    // a Landing that did not land — the same shape, so that the one way out is
    // the same way out.
    let failed = |error| Landing {
        outcome: Err(error),
        incoming: incoming.email().to_string(),
        incoming_is_live: false,
    };

    if let Err(error) = refuse_a_shared_profile(incoming, registry) {
        return failed(error);
    }

    let store = match registry::the_default_profile(host) {
        Ok(ground) => ground,
        Err(error) => return failed(error),
    };

    let mut incoming_is_live = false;
    let switched: Result<Captured> = lock::under(host, probe::locks_for(&store), |held| {
        let prepared = prepare(host, incoming, outgoing, installed.clone(), store)?;

        // Every step of the Switch is slow enough to outlast a hold. `prepare`
        // reads a Credential and `capture` reads and writes one, and a keychain
        // that stops to ask the user for permission stretches either without
        // warning — past the ten seconds the config-file lock goes stale in.
        let mut holds = lock::Holds::of(held, perch);

        let captured = holds
            .around(|| capture(host, &prepared, incoming, outgoing, registry))
            .map_err(|error| error.with_note(&nothing_happened(outgoing)))?;

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
        incoming_is_live,
    }
}

/// Makes an Account's Credential the live one without Capturing what it
/// replaces.
///
/// Every Switch Captures first, because the live Credential is the only good
/// copy of the outgoing Account's (ADR 0006). A `perch relogin` of the Account
/// you are on is the one case where that is false in both directions: the
/// Credential about to be replaced belongs to the Account being repaired, and a
/// login has already replaced it. Capturing here would write the broken copy
/// over the fresh one — the one mistake this design cannot recover from,
/// arriving as tidiness.
///
/// The Credential written is the one in the Account's own Profile, read back
/// out of it rather than passed in, so the same store that a `perch switch`
/// tomorrow will read is the store this proves works today.
///
/// It asks the liveness question under the locks, as [`perform`] does. The
/// caller has asked it too, before the browser round trip and again after — but
/// both of those are minutes and a lock wait away from the write, and this is
/// the last moment at which the answer cannot change.
///
/// `whose` is the sentence saying why the Default Profile is the wrong thing to
/// overwrite, and is the caller's for the reason [`refuse_if_live_anywhere`]
/// gives about its own: what makes it wrong is different for a repair and for a
/// removal, and the refusal is read by somebody deciding which client to quit.
/// Written in here, it told a `perch remove` about "this Account's repaired
/// Credential" — nothing was being repaired, and what lands is the successor's.
pub fn make_live(
    host: &dyn Host,
    perch: &mut lock::Held<'_>,
    account: &Account,
    whose: &str,
) -> std::result::Result<(), NotLanded> {
    let (installed, store) = ground(host).map_err(|error| NotLanded {
        error,
        is_live: false,
    })?;

    let mut is_live = false;
    let landed = lock::under(host, probe::locks_for(&store), |held| {
        // Under the locks, for the reason [`Prepared`] gives and `perch
        // relogin` had no equivalent of: the caller asked this question minutes
        // ago, across a browser round trip, and taking these locks can take
        // seconds more. A `claude` started in that gap is one no earlier answer
        // saw, and what happens next replaces the very Credential it is holding
        // — the mid-task logout ADR 0005 exists to prevent, reached by the one
        // path that writes the Default Profile without Capturing first.
        //
        // The Default Profile alone. The Account's own Profile is only read
        // here, and reading a Credential takes nothing away from the session
        // using it (ADR 0027).
        refuse_if_live_in(host, &store.config_dir, whose, &installed)?;

        let prepared = prepare(host, account, None, installed, store)?;
        let mut holds = lock::Holds::of(held, perch);

        holds.around(|| {
            profile::store_credential(host, &prepared.store, prepared.credential.as_str())
        })?;
        is_live = true;
        holds.around(|| patch_identity(host, &prepared))
    });

    landed.map_err(|error| NotLanded { error, is_live })
}

/// A `make_live` that stopped part way, and what the machine is holding now.
///
/// The same distinction [`Landing`] draws, for the same reason: a failure
/// after the Credential was written but before the Identity was patched has
/// still changed which Account the machine is acting as, and a caller that
/// records who is active has to record what is true rather than what it asked
/// for.
pub struct NotLanded {
    pub error: PerchError,
    /// Whether the Account's Credential is the live one despite the failure.
    pub is_live: bool,
}

/// Whether the machine already says what a Switch to this Account would make it
/// say — in both the places that have to agree.
///
/// A Switch is only complete when the live Credential and the Identity name the
/// same Account. One interrupted between those two steps leaves them
/// disagreeing, and running the same command again is how it is repaired: so
/// this asks Claude Code's own file rather than only Perch's record of who is
/// active, or the repair would be turned away as unnecessary.
///
/// Both, and not the Identity alone. `claude /logout` empties the live store
/// and leaves `.claude.json` naming whoever was there — so an Identity read on
/// its own says a Switch has already landed onto a machine that is logged out,
/// and `perch switch <that account>` is exactly the command that would put it
/// back. It is the same shape of half-state as the interrupted Switch, reached
/// from the other side, and it wants the same answer.
pub fn already_landed(host: &dyn Host, installed: &Installed, account: &Account) -> Result<bool> {
    let store = registry::the_default_profile(host)?;
    // Read the way the Credential beside it is read, and for the same reason
    // spelled out there. An Identity Perch cannot understand — an `oauthAccount`
    // with no `emailAddress`, a `.claude.json` that is not JSON, an address with
    // nothing nameable in it — is a file naming nobody, so nothing has landed,
    // and `perch switch <the active Account>` is precisely the command that
    // rewrites it. Propagated, it refused that repair on the strength of the
    // file the repair exists to replace, while a Switch to any *other* Account
    // went through: `capture` swallows the same failure and `patch_oauth_account`
    // only needs the file to be a JSON object.
    let named = probe::read_identity(host, &store, installed)
        .ok()
        .flatten()
        .is_some_and(|identity| registry::same_name(&identity.email, account.email()));

    // A live store holding bytes that are not a Credential has not landed
    // anywhere: Claude Code cannot use them, and the Switch this would turn
    // away is the one that writes a good Credential over the bad one. That is
    // the same half-state as the interrupted Switch above, reached from a third
    // side, and it wants the same answer — so an unreadable store is `false`
    // rather than an error. Propagating it refused every Switch on the machine,
    // including the repair, on the strength of a file it was about to replace.
    let usable = matches!(probe::read_credential(host, &store, installed), Ok(Some(_)));

    Ok(named && usable)
}

/// The two things that are true whatever else is: which Claude Code is
/// installed, and where the Default Profile is. Established before the locks,
/// because the locks are derived from the second of them.
fn ground(host: &dyn Host) -> Result<(Installed, Store)> {
    Ok((
        Installed::probed(host)?,
        registry::the_default_profile(host)?,
    ))
}

/// Refuses a Switch onto an Account whose Profile is not its alone.
///
/// A Profile is `profiles/<slugged email>`, and the slug flattens every
/// non-alphanumeric character — so `some-one@example.com` and
/// `some.one@example.com` name one directory, one Credential Store, and one
/// Credential between them. `perch add` refuses to make that state and `perch
/// remove` degrades rather than deleting into it; this was the consumer with no
/// guard at all.
///
/// What a Switch would do there is the one disagreement ADR 0006 exists to keep
/// impossible. `prepare` reads the shared store and gets whichever Account's
/// Credential is in it, `store_credential` writes *that* into the Default
/// Profile, and `patch_identity` then writes the Identity of the Account that
/// was asked for. The machine acts as one Account while Claude Code displays
/// the other and the registry records the other, and nothing afterwards can
/// tell which of the two the live Credential belongs to.
fn refuse_a_shared_profile(incoming: &Account, registry: &Registry) -> Result<()> {
    let Some(sharer) = registry.accounts.iter().find(|held| {
        held.email() != incoming.email() && registry::same_profile(held.email(), incoming.email())
    }) else {
        return Ok(());
    };
    Err(PerchError::Conflict(format!(
        "{} and {} share one Profile, so they share one Credential — their \
         addresses differ only in characters a Profile directory does not keep \
         apart.\n\
         Switching would write whichever Credential that Profile holds and then \
         name the other Account as the one it belongs to, and nothing could tell \
         them apart afterwards. `perch remove` one of them, then `perch add` it \
         again.",
        incoming.email(),
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
    // Before anything is written, and only of the Profile that is written to.
    //
    // The Capture writes the live Credential into the outgoing Account's
    // Profile, and a client running there is holding that file — writing under
    // it is the mid-task logout ADR 0005 exists to prevent. The incoming
    // Account's Profile is only ever *read* from, and reading a Credential to
    // copy it into the Default Profile takes nothing away from the session that
    // is using it (ADR 0027). Refusing that as well would make a Run and a
    // Switch lock each other out for no reason: an Account you are running in
    // one terminal is exactly the Account you would want active in the others.
    if let Some(outgoing) = outgoing {
        refuse_if_live(host, outgoing, &installed)?;
    }

    // From whichever of the Profile's two Credential Stores holds one (ADR
    // 0020): an Account is switchable to as long as its Credential is
    // somewhere Claude Code would have looked.
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

/// Step one: the live Credential goes back where it belongs.
///
/// "Where it belongs" is the part worth being careful about. Perch is not the
/// only thing that writes the Default Profile: somebody who runs `claude` and
/// logs in directly leaves Perch's record of who is active behind, and the live
/// Credential then belongs to a login Perch never made. Writing it into the
/// outgoing Account's Profile would destroy that Account's own Credential — and
/// worse than lose it, because a later Switch back would make the stranger's
/// Credential live while the Identity was patched to name the Account Perch
/// thinks it is, so Claude Code would act as one person while displaying
/// another.
///
/// The evidence is the machine's own, and the same [`already_landed`] and
/// [`only_off_a_credential_that_is_theirs`] read: Claude Code writes the
/// Credential and the Identity beside it together, so a `.claude.json` naming
/// the outgoing Account says the live Credential is theirs. An Identity that is
/// absent or cannot be read is *not* evidence against, and still Captures —
/// losing a Rotation is the failure this step exists to prevent.
///
/// The Identity is only ever asked about a Credential that could be somebody's
/// to lose. One identical to the Credential this Switch is about to write is the
/// incoming Account's by construction, and is declined before the Identity gets
/// a say — an Identity left behind by an interrupted Switch names the wrong
/// Account, and believing it there is how the Capture would destroy the outgoing
/// Account's Credential rather than save one.
///
/// [`only_off_a_credential_that_is_theirs`]: crate::observe
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

    // Bytes that are not a Credential are not a Rotation, and the point of a
    // Capture is not losing a Rotation. So a live store holding rubbish is
    // declined rather than propagated: filing it under the outgoing Account
    // would overwrite the Credential that Account does have with rubbish, and
    // failing here would stop every Switch on the machine — including the one
    // that repairs it by writing a good Credential over the bad one.
    //
    // A store that *would not answer* is the other thing entirely, and folding
    // the two together is how a Capture came to be skipped over a Credential
    // that was never established to be bad. A `.credentials.json` left owned by
    // root by a `sudo claude`, or a keychain that will not open, refuses the
    // read and says nothing about what it holds — which is very likely the
    // outgoing Account's own Credential, several Rotations newer than the copy
    // in its Profile. Declined there, the Switch went straight on to write the
    // incoming Credential over it, and the only good copy was gone. So it is
    // refused: nothing has been written yet, and a Switch that has to be run
    // again after a `chmod` is recoverable where a lost refresh token is not.
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

    // The live Credential being byte-for-byte the one this Switch is about to
    // write is the trace of a Switch that stopped after step two: the incoming
    // Account's Credential is already live, and what is behind is `.claude.json`,
    // Perch's record of who is active, or both. There is no Rotation in it to
    // save, and it is not the outgoing Account's whoever the Identity names.
    //
    // Ahead of the Identity, because a stale Identity is the whole of what that
    // state *is*: it names the Account the interrupted Switch was leaving. Read
    // as evidence of ownership it says the live Credential is the outgoing
    // Account's, and the write below then files the *incoming* Account's
    // Credential into the outgoing Account's Profile — over the only copy of a
    // refresh token that Account had, which is the one loss ADR 0006 exists to
    // prevent. Running the same `perch switch` again after it failed to record
    // itself is enough to reach it.
    if live.as_str() == prepared.credential.as_str() {
        return Ok(Captured::NothingToSave);
    }

    // `incoming` and `outgoing` being one Account is the repair for a Switch that
    // stopped between step two and step three, and the check above has already
    // taken the case where it has nothing to do. ADR 0027 expects the Capture to
    // run here — "X is the outgoing Account there as well as the incoming one".
    //
    // What is left is a live Credential that is this Account's Profile copy
    // neither, and there are two readings — this Account Rotated while it was
    // live, or somebody logged in outside Perch since. Nothing on the machine
    // tells them apart, so neither is acted on. Nothing has been written yet, and
    // a Switch that has to be run again is recoverable where a lost refresh token
    // is not.
    if registry::same_name(incoming.email(), outgoing.email()) {
        return Err(PerchError::Conflict(
            the_live_credential_is_unaccounted_for(incoming),
        ));
    }

    // Over the whole of Unicode, as every other comparison of two addresses is.
    // `.claude.json` is Claude Code's file and nothing makes it agree with the
    // registry about the case of a letter outside ASCII, so under ASCII folding
    // this read `CAFÉ@example.com` and `café@example.com` as two different
    // people, declined the Capture, and let the write below destroy the
    // Rotation it had just declined to save.
    if let Ok(Some(identity)) = probe::read_identity(host, &prepared.store, &prepared.installed)
        && !registry::same_name(&identity.email, outgoing.email())
    {
        // And corroborated, because an Identity naming somebody else is the one
        // piece of evidence here that Perch does not write and that goes stale
        // in a state Perch itself produces. `Landing::record` files the incoming
        // Account as active whenever its Credential went live, including when
        // step three failed — so a Switch to B whose `patch_identity` failed
        // leaves the registry saying B and `.claude.json` still saying A. B then
        // runs and Rotates. On the next `perch switch C`, this branch read A,
        // declined the Capture, and the write below put C's Credential over B's
        // Rotation: the one loss ADR 0006 exists to prevent, reached by
        // believing a file Perch had just failed to update. The report even said
        // the live Credential was A's and "not kept", which was untrue.
        //
        // So the Identity is decisive only where something else agrees with it.
        // An address Perch does not hold is a login made outside Perch, whose
        // Credential is nobody's to file. An address Perch does hold, whose own
        // stored copy *is* what is live, is a machine that really is on that
        // Account. Anything else is unaccounted for, and gets what the same
        // ambiguity gets above: a refusal, with nothing written.
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
/// [`Corroboration::Unaccounted`]: this decides whether a refresh token is about
/// to be written over, and "the keychain was locked" is not evidence that it is
/// safe to.
fn corroborates(
    host: &dyn Host,
    registry: &Registry,
    outgoing: &Account,
    named: &str,
    live: &str,
) -> Corroboration {
    // Asked first, and of the outgoing Account rather than of the one named. It
    // is the question with something at stake in it: a Capture exists to save a
    // Rotation, and where the live Credential is already the copy that Profile
    // holds there is no Rotation and nothing a wrong answer could cost. That is
    // the ordinary interrupted Switch — Perch's record moved on and
    // `.claude.json` did not — and it must stay a Switch that simply runs.
    if held_by(host, outgoing).is_some_and(|held| held == live) {
        return Corroboration::NothingAtStake;
    }
    let Some(account) = registry.account(named) else {
        return Corroboration::NotOurs;
    };
    match held_by(host, account) {
        Some(held) if held == live => Corroboration::NotOurs,
        _ => Corroboration::Unaccounted,
    }
}

/// What an Account's own Profile holds, where it can be read at all.
fn held_by(host: &dyn Host, account: &Account) -> Option<String> {
    let store = account.store(host).ok()?;
    Some(credentials::read(host, &store).ok()??.credential)
}

/// The refusal for a live Credential an Identity names somebody else for, where
/// that somebody else is an Account Perch holds and is not holding this.
fn the_identity_is_not_corroborated(outgoing: &Account, named: &str) -> String {
    format!(
        "The Identity beside the live Credential names {named}, but the live \
         Credential is not the one Perch holds for {named} either — and Perch is \
         on {}, so it cannot tell whose Rotation this is.\n\
         It may be {}'s, made after a Switch that could not finish writing the \
         Identity: writing over it would destroy the only good copy, so nothing \
         was changed.\n\
         It may be {named}'s, Rotated since. `perch switch {named}` files it \
         under the Account the Identity names; `perch relogin` replaces it with \
         a fresh login for whichever Account you meant.",
        outgoing.email(),
        outgoing.email(),
    )
}

/// What Perch cannot establish, when the repair for an interrupted Switch finds a
/// live Credential that is not the one it holds.
///
/// Both readings named, because the remedies are different and the user is the
/// only one who knows which happened. `perch relogin` is the way through either
/// way: it replaces whatever is live with a fresh login for this Account, which
/// costs nothing if the live Credential was this Account's own.
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
/// live — that key of it, and nothing else of it (ADR 0001).
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

/// The `oauthAccount` block for an Account.
///
/// Its own Profile holds the block Claude Code wrote when that Account logged
/// in, which carries fields beyond the Identity Perch records — so that block
/// is preferred verbatim, and one is composed only for the Accounts that have
/// none, such as the login adoption took over (ADR 0009).
fn identity_block_for(host: &dyn Host, incoming: &Account) -> Result<String> {
    let kept = incoming.store(host)?.identity_file;
    let held = host
        .read_file(&kept)
        .ok()
        .and_then(|contents| probe::oauth_account_block(&contents).map(str::to_string));

    Ok(held.unwrap_or_else(|| incoming.identity.oauth_account_block()))
}

/// Refuses to touch a Profile something else is holding (ADR 0005).
///
/// Public because two callers ask it *before* they spend something rather than
/// after. `perch relogin` asks before a browser round trip — a Profile Perch
/// may not write to is one no login was ever going to repair — and `perch
/// watcher run` asks before it reads every candidate's Utilization, because a
/// Switch that is going to be refused is a Switch whose candidates never needed
/// ranking.
pub fn refuse_if_live(host: &dyn Host, account: &Account, installed: &Installed) -> Result<()> {
    refuse_if_live_in(
        host,
        &account.profile_dir(host)?,
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
) -> Result<()> {
    let running = probe::live_clients(host, config_dir, installed)?;
    if running.is_empty() {
        return Ok(());
    }

    let pids: Vec<String> = running.iter().map(u32::to_string).collect();
    Err(PerchError::ProfileLive(format!(
        "A client is running against {whose} (pid {}).\n\
         Nothing was changed. That Credential belongs to it until it exits — \
         quit it, or switch to a different Account.",
        pids.join(", ")
    )))
}

/// Both Profiles a command may write into, refused while a client is holding
/// either.
///
/// One place, because every command that needs this asks it twice — once before
/// it puts an unbounded wait to the person, and once after, because the first
/// answer is about a machine that has since had minutes to move on. Two
/// spellings of the same pair of checks is how the second ask comes to be
/// weaker than the first, and the second ask is the one standing between a
/// running session and a Credential written out from under it.
///
/// `the_default_profile_too` is the sentence saying why the Default Profile is
/// written as well, and `None` is a command that leaves it alone. Named rather
/// than derived, because what makes the Default Profile the wrong thing to
/// overwrite is different for a repair and for a removal, and the refusal is
/// read by somebody deciding which client to quit.
pub fn refuse_if_live_anywhere(
    host: &dyn Host,
    account: &Account,
    the_default_profile_too: Option<&str>,
    installed: &Installed,
) -> Result<()> {
    refuse_if_live(host, account, installed)?;

    if let Some(whose) = the_default_profile_too {
        // Its Credential is the one a running client is holding, and this would
        // replace it rather than renew it — the mid-task logout ADR 0005 exists
        // to prevent.
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
                enabled: true,
                quarantine: None,
                group: None,
                utilization: None,
            });
        }
        registry.active = Some(OUTGOING.to_string());
        registry
    }

    fn landing(outcome: Result<Captured>, incoming_is_live: bool) -> Landing {
        Landing {
            outcome,
            incoming: INCOMING.to_string(),
            incoming_is_live,
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
    /// than one at a time — the pair that diverged between `perch switch` and
    /// `perch watcher run` was a *combination*, and each half of it was
    /// covered.
    ///
    /// The fifth row is the one no `perform` produces today: a Quarantine
    /// diagnosed after the Credential was written. Nothing after
    /// `store_credential` raises `Quarantined`, so it is unreachable — and that
    /// was exactly the invariant one caller silently relied on and no test
    /// stated. `record` answers it the same way for everybody, so a fourth step
    /// in `perform` cannot reopen it.
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
                what: "a Switch that failed before it wrote anything",
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
                registry.active.as_deref(),
                Some(case.active),
                "{}: which Account is active is a fact about which Credential is live",
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

    /// The failure that got us here is the one the user reads and the one the
    /// exit code comes from. A `record` that replaced it with its own would cost
    /// a script the code it branches on.
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

    /// A registry Perch could not write is not worth losing the failure over:
    /// the worst a missed Quarantine costs is making the same discovery next
    /// time, and the failure already says the Account has to be logged into
    /// again.
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
