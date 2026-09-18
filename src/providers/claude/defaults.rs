//! Claude Default credentials and identity changes (ADR a-switch-is-written-down-first).

use super::profile;
use crate::domain::Quarantine;
use crate::providers::claude::probe::{Credential, Installed, Store};
use crate::providers::claude::{credentials, probe};
use crate::providers::provider::{
    Captured, DefaultChange, DefaultFailure, DefaultRequest, ProfileRef as Account,
};
use crate::{Host, PerchError, Result, host, live, lock, name};
use zeroize::Zeroizing;

struct Prepared<'h> {
    installed: Installed<'h>,
    store: Store,
    credential: Credential,
    /// The `oauthAccount` block to write, ready to splice in.
    identity_block: String,
}

pub(super) fn begin<'h>(
    host: &'h dyn Host,
    perch: &mut lock::Held<'_>,
    request: DefaultRequest,
    installed: &Installed<'h>,
) -> Result<Box<dyn DefaultChange + 'h>> {
    let store = crate::providers::claude::layout::default_profile(host)?;
    let mut native = store.seized(host)?;
    let mut holds = lock::Holds::of(&mut native, perch);
    if let Some(whose) = &request.overwrite {
        holds.around(|| {
            live::ask(
                host,
                &[live::Place::new(
                    crate::providers::provider::Id::Claude,
                    whose.clone(),
                    &store.config_dir,
                )],
            )
            .idle_or(&live::NOTHING_WAS_CHANGED)
        })?;
    }
    let prepared = holds.around(|| {
        prepare(
            host,
            &request.incoming,
            request.outgoing.as_ref(),
            installed.clone(),
            &store,
        )
    })?;
    Ok(Box::new(Edit {
        host,
        native,
        prepared,
        request,
    }))
}

struct Edit<'h> {
    host: &'h dyn Host,
    native: lock::Held<'h>,
    prepared: Prepared<'h>,
    request: DefaultRequest,
}
impl DefaultChange for Edit<'_> {
    fn capture(&mut self, perch: &mut lock::Held<'_>) -> Result<Captured> {
        lock::Holds::of(&mut self.native, perch).around(|| {
            capture(
                self.host,
                &self.prepared,
                &self.request.incoming,
                self.request.outgoing.as_ref(),
                &self.request.known,
            )
        })
    }
    fn apply(&mut self, perch: &mut lock::Held<'_>) -> std::result::Result<(), DefaultFailure> {
        let mut holds = lock::Holds::of(&mut self.native, perch);
        holds
            .around(|| {
                profile::store_credential(
                    self.host,
                    &self.prepared.store,
                    self.prepared.credential.as_str(),
                )
            })
            .map_err(|error| DefaultFailure {
                error,
                moved: false,
            })?;
        holds
            .around(|| patch_identity(self.host, &self.prepared))
            .map_err(|error| DefaultFailure {
                error: error.with_note(&live_but_unnamed(
                    self.request.outgoing.as_ref(),
                    &self.request.incoming,
                )),
                moved: true,
            })
    }
}
fn prepare<'h>(
    host: &dyn Host,
    incoming: &Account,
    outgoing: Option<&Account>,
    installed: Installed<'h>,
    store: &Store,
) -> Result<Prepared<'h>> {
    // Before anything is written, and only of the Profile written to. The
    // incoming Account's is only ever read from, and reading takes nothing away
    // from the session using it.
    if let Some(outgoing) = outgoing {
        live::ask(
            host,
            &[live::Place::new(
                crate::providers::provider::Id::Claude,
                format!("{}'s Profile", outgoing.key()),
                outgoing.directory(),
            )],
        )
        .idle_or(&live::NOTHING_WAS_CHANGED)?;
    }

    // Derived once and read twice: a derivation is a `PERCH_HOME` lookup, a
    // slug, a component walk and a SHA-256 of the result.
    let incoming_store = incoming.store(host)?;

    // From whichever of the Profile's two Credential Stores holds one: an
    // Account is switchable to as long as its Credential is somewhere Claude
    // Code would have looked.
    let held =
        credentials::read(host, &incoming_store)?.ok_or_else(|| PerchError::Quarantined {
            why: Quarantine::NoCredential,
            said: format!(
                "Perch holds no Credential for {}, so it is Quarantined. Run `perch relogin {}` to repair it.",
                incoming.key(), incoming.key(),
            ),
        })?;
    let credential = probe::understand_credential(
        held.credential,
        &format!("the Credential Perch holds for {}", incoming.key()),
        &installed,
    )?;

    Ok(Prepared {
        identity_block: identity_block_for(host, incoming, &incoming_store)?,
        installed,
        store: store.clone(),
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
    profiles: &[Account],
) -> Result<Captured> {
    let Some(outgoing) = outgoing else {
        return Ok(Captured::NoOutgoing);
    };

    // Bytes that are not a Credential are not a Rotation, so a live store
    // holding rubbish is declined. One that *would not answer* says nothing
    // about what it holds and is refused, with nothing written.
    let live = match probe::read_credential(host, &prepared.store, &prepared.installed) {
        Ok(live) => live,
        Err(why @ PerchError::ProbeRefused(_)) => {
            return Ok(Captured::Unreadable {
                outgoing: outgoing.key().to_string(),
                why: why.to_string(),
            });
        }
        Err(would_not_answer) => {
            return Err(would_not_answer.with_note(&format!(
                "The live Credential could not be read, so it was not Captured \
                 for {}. Make that store readable and run this again.",
                outgoing.key(),
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
    if name::same_name(incoming.key(), outgoing.key()) {
        return Err(PerchError::Conflict(
            the_live_credential_is_unaccounted_for(incoming),
        ));
    }

    let identity = probe::read_identity(host, &prepared.store, &prepared.installed)
        .ok()
        .flatten();
    if outgoing.provider_identity.is_some() && identity.is_none() {
        if held_by(host, outgoing).is_some_and(|held| *held == live.as_str()) {
            return Ok(Captured::NothingToSave);
        }
        return Err(PerchError::Conflict(
            "The live Claude Credential has no readable identity. Nothing was captured; restore the native identity or relogin before switching.".into(),
        ));
    }
    if let Some(identity) = identity
        && !super::identity::names(&identity, outgoing)
    {
        // The Identity is decisive only where something else agrees with it: it
        // is the one piece of evidence here Perch does not write, and it goes
        // stale in a state Perch itself produces.
        return match corroborates(host, profiles, outgoing, &identity, live.as_str()) {
            Corroboration::NothingAtStake => Ok(Captured::NothingToSave),
            Corroboration::NotOurs => Ok(Captured::NotTheirs {
                outgoing: outgoing.key().to_string(),
                live: identity.email,
            }),
            Corroboration::Unaccounted => Err(PerchError::Conflict(
                the_identity_is_not_corroborated(outgoing, &identity.email),
            )),
        };
    }

    // A Run points a client at the Account's own Profile (ADR a-run-is-one-shot),
    // so a Rotation there leaves the live copy the older of the two and
    // Capturing it would retire the newer.
    let store = outgoing.store(host)?;
    if let Ok(Some(held)) = probe::read_credential(host, &store, &prepared.installed)
        && supersedes(&held, &live)
    {
        return Ok(Captured::Superseded {
            outgoing: outgoing.key().to_string(),
        });
    }
    profile::store_credential(host, &store, live.as_str())?;

    Ok(Captured::Copied {
        from: outgoing.key().to_string(),
    })
}

/// Whether the copy an Account's own Profile holds is newer than the live one.
///
/// `expiresAt` is what a Rotation moves. Strictly later, and only where both say
/// so: a Credential silent about its expiry is no evidence.
fn supersedes(held: &Credential, live: &Credential) -> bool {
    matches!(
        (held.expires_at, live.expires_at),
        (Some(held), Some(live)) if held > live
    )
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
    profiles: &[Account],
    outgoing: &Account,
    named: &crate::domain::Identity,
    live: &str,
) -> Corroboration {
    // Asked first, and of the outgoing Account rather than the one named: it is
    // the question with something at stake, and where the live Credential is
    // already that Profile's copy there is no Rotation to lose.
    if held_by(host, outgoing).is_some_and(|held| *held == live) {
        return Corroboration::NothingAtStake;
    }
    let Some(account) = profiles
        .iter()
        .find(|account| super::identity::names(named, account))
    else {
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
    let outgoing = outgoing.key();
    format!(
        "The Identity beside the live Credential names {named}, and Perch holds \
         that Credential for neither {named} nor {outgoing}, the Account it is \
         on. It may be {outgoing}'s from a Switch that could not finish, or \
         {named}'s, Rotated since.\n\
         `perch relogin {outgoing}` files the live Credential under the Account \
         Perch is on."
    )
}

/// What Perch cannot establish, when the repair for an interrupted Switch finds
/// a live Credential that is not the one it holds. Both readings are named,
/// because the remedies differ and `perch relogin` is the way through either
/// way.
fn the_live_credential_is_unaccounted_for(account: &Account) -> String {
    let email = account.key();
    format!(
        "{email} is the Account Perch is on and the Account asked for, but the \
         live Credential is not the one Perch holds for it. It may be {email}'s \
         own, Rotated since, or a login made outside Perch.\n\
         `perch relogin {email}` finishes the repair. To keep a login made \
         outside Perch instead, `perch add` holds it as an Account of its own \
         first."
    )
}

/// Step three: `.claude.json` comes to name the Account whose Credential is now
/// live — that key of it, and nothing else of it
/// (ADR everything-but-the-account).
fn patch_identity(host: &dyn Host, prepared: &Prepared) -> Result<()> {
    let file = &prepared.store.identity_file;
    let patched = match host.read_file(file).map(Zeroizing::new) {
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
            crate::secret::Secret::taken_over(probe::fresh_identity_file(&prepared.identity_block))
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
fn identity_block_for(host: &dyn Host, incoming: &Account, kept_in: &Store) -> Result<String> {
    let held = host
        .read_file(&kept_in.identity_file)
        .map(Zeroizing::new)
        .ok()
        .and_then(|contents| probe::oauth_account_block(&contents).map(str::to_string));

    Ok(held.unwrap_or_else(|| super::identity::compose(&incoming.identity)))
}

fn live_but_unnamed(outgoing: Option<&Account>, incoming: &Account) -> String {
    let named = match outgoing {
        Some(outgoing) => outgoing.key().to_string(),
        None => "another Account".to_string(),
    };
    format!(
        "{incoming} is active, but Claude Code still displays {named}.\n\
         `perch switch {incoming}` again finishes the job.",
        incoming = incoming.key(),
    )
}

pub(super) fn already_landed(
    host: &dyn Host,
    installed: &Installed,
    account: &Account,
) -> Result<bool> {
    let store = crate::providers::claude::layout::default_profile(host)?;
    // An Identity Perch cannot understand is a file naming nobody, so nothing
    // has landed, and `perch switch <the active Account>` is the command that
    // rewrites it. Propagated, it would refuse the repair it exists for.
    let named = probe::read_identity(host, &store, installed)
        .ok()
        .flatten()
        .is_some_and(|identity| super::identity::names(&identity, account));

    // A live store holding bytes that are not a Credential has landed nowhere,
    // and the Switch this would turn away is the one that writes a good
    // Credential over the bad one. So `false` rather than an error.
    let usable = matches!(probe::read_credential(host, &store, installed), Ok(Some(_)));

    Ok(named && usable)
}

pub(super) fn inspect(
    host: &dyn Host,
) -> Result<Box<dyn crate::providers::provider::DefaultInspection + '_>> {
    let store = crate::providers::claude::layout::default_profile(host)?;
    let native = store.seized(host)?;
    Ok(Box::new(Inspection {
        host,
        store,
        native,
    }))
}
struct Inspection<'h> {
    host: &'h dyn Host,
    store: Store,
    native: lock::Held<'h>,
}
impl crate::providers::provider::DefaultInspection for Inspection<'_> {
    fn resolve(
        &mut self,
        perch: &mut lock::Held<'_>,
        profiles: &[Account],
        leaving: Option<&str>,
        arriving: &str,
        may_continue: &mut dyn FnMut() -> bool,
    ) -> Result<crate::providers::provider::DefaultObservation> {
        use crate::providers::provider::DefaultObservation;
        let mut holds = lock::Holds::of(&mut self.native, perch);
        if !may_continue() {
            return Ok(DefaultObservation::Stopped);
        }
        let live = holds.around(|| credentials::read(self.host, &self.store)).map_err(|error| error.with_note(&format!(
            "A Switch to {arriving} was in flight and was not recorded, and the live Credential is the only thing that says whether it happened.\nMake that store readable and run this again."
        )))?;
        let Some(live) = live else {
            return Ok(DefaultObservation::Settled(leaving.map(str::to_string)));
        };
        let named = [Some(arriving), leaving]
            .into_iter()
            .flatten()
            .filter_map(|key| {
                profiles
                    .iter()
                    .find(|profile| name::same_name(profile.key(), key))
            });
        let rest = profiles.iter().filter(|profile| {
            !name::same_name(profile.key(), arriving)
                && !leaving.is_some_and(|key| name::same_name(profile.key(), key))
        });
        for profile in named.chain(rest) {
            if !may_continue() {
                return Ok(DefaultObservation::Stopped);
            }
            if let Some(held) = holds.around(|| held_by(self.host, profile))
                && *held == *live.credential
            {
                return Ok(DefaultObservation::Settled(Some(profile.key().to_string())));
            }
        }
        Ok(DefaultObservation::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;
    use crate::providers::claude::probe::Identity;
    use crate::registry::Registry;
    const INCOMING: &str = "incoming@example.com";
    const OUTGOING: &str = "outgoing@example.com";
    fn two_accounts() -> Registry {
        let mut registry = Registry::default();
        for email in [OUTGOING, INCOMING] {
            registry.upsert(crate::registry::Account {
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

    fn profiles(host: &dyn Host, registry: &Registry) -> Vec<Account> {
        registry
            .accounts
            .iter()
            .map(|account| account.profile(host).unwrap())
            .collect()
    }
    /// A parseable Credential, distinct per `tag`; `expires_at` is what a
    /// Rotation moves, so it is the axis these fixtures vary on.
    fn credential_json(tag: &str, expires_at: Option<i64>) -> String {
        let expiry = match expires_at {
            Some(at) => format!(",\"expiresAt\":{at}"),
            None => String::new(),
        };
        format!(
            "{{\"claudeAiOauth\":{{\"accessToken\":\"sk-ant-oat01-{tag}\",\
             \"refreshToken\":\"sk-ant-ort01-{tag}\"{expiry}}}}}"
        )
    }

    fn understood(json: &str) -> Credential {
        probe::understand_credential(
            Zeroizing::new(json.to_string()),
            "a fixture Credential",
            &Installed::unknown("2.1.221"),
        )
        .expect("the fixture is a Credential")
    }

    fn a_home() -> FakeHost {
        FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("USER", "someone")
    }

    fn prepared_to_write(host: &FakeHost, incoming_credential: &str) -> Prepared<'static> {
        Prepared {
            installed: Installed::unknown("2.1.221"),
            store: crate::providers::claude::layout::default_profile(host).expect("home is known"),
            credential: understood(incoming_credential),
            identity_block: String::new(),
        }
    }

    fn write_live(host: &FakeHost, json: &str) {
        let store = crate::providers::claude::layout::default_profile(host).expect("home is known");
        let [primary, _] = credentials::stores_for(host, &store);
        primary.write(host, json).expect("the store takes it");
    }

    fn write_own(host: &FakeHost, registry: &Registry, email: &str, json: &str) {
        let store = registry
            .account(email)
            .expect("the fixture holds it")
            .profile(host)
            .unwrap()
            .store(host)
            .expect("home is known");
        let [primary, _] = credentials::stores_for(host, &store);
        primary.write(host, json).expect("the store takes it");
    }

    fn one_of(registry: &Registry, email: &str) -> Account {
        registry
            .account(email)
            .expect("the fixture holds it")
            .profile(&a_home())
            .unwrap()
    }

    /// Asked again once the native locks are taken, because a Watcher told to
    /// stop between the ask outside them and this one reads nothing further.
    #[test]
    fn a_landing_resolved_after_the_watch_went_reads_no_store() {
        use crate::providers::provider::DefaultObservation;
        let host = a_home();
        write_live(&host, &credential_json("live", None));
        let mut perch = crate::holdings::lock(&host).expect("nobody holds it");
        let mut inspection = inspect(&host).expect("nobody holds the native locks");

        let stopped = inspection
            .resolve(&mut perch, &[], None, INCOMING, &mut || false)
            .expect("a stop is an answer rather than a failure");
        let went_on = inspection
            .resolve(&mut perch, &[], None, INCOMING, &mut || true)
            .expect("the same machine, asked to go on");

        assert!(matches!(stopped, DefaultObservation::Stopped));
        assert!(
            matches!(went_on, DefaultObservation::Unknown),
            "the live store holds a Credential no Profile here accounts for"
        );
    }

    #[test]
    fn a_copy_supersedes_only_where_both_expiries_are_said_and_the_held_is_later() {
        let earlier = understood(&credential_json("live", Some(1_000)));
        let later = understood(&credential_json("held", Some(2_000)));
        let silent = understood(&credential_json("silent", None));

        assert!(supersedes(&later, &earlier));
        assert!(!supersedes(&earlier, &later));
        assert!(
            !supersedes(&earlier, &earlier),
            "equal is not strictly later"
        );
        assert!(!supersedes(&silent, &earlier), "silence is no evidence");
        assert!(!supersedes(&later, &silent));
    }

    #[test]
    fn an_unidentified_live_credential_is_not_captured_into_a_stable_subject() {
        let host = a_home();
        let registry = two_accounts();
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));
        let mut outgoing = one_of(&registry, OUTGOING);
        outgoing.provider_identity = Some(
            crate::providers::provider::AccountIdentity::new(
                crate::providers::provider::Id::Claude,
                "user".into(),
                "workspace".into(),
            )
            .unwrap(),
        );
        write_live(&host, &credential_json("unknown", Some(2_000)));
        let error = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            Some(&outgoing),
            &profiles(&host, &registry),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("no readable identity"),
            "{error}"
        );
        assert!(held_by(&host, &outgoing).is_none());
    }

    #[test]
    fn a_capture_with_no_outgoing_has_nothing_to_save_into() {
        let host = a_home();
        let registry = two_accounts();
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));

        let captured = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            None,
            &profiles(&host, &registry),
        )
        .expect("nothing to do is not a failure");

        assert_eq!(captured, Captured::NoOutgoing);
    }

    #[test]
    fn a_capture_with_nothing_live_saves_nothing() {
        let host = a_home();
        let registry = two_accounts();
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));

        let captured = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            Some(&one_of(&registry, OUTGOING)),
            &profiles(&host, &registry),
        )
        .expect("a logged-out machine is not a failure");

        assert_eq!(captured, Captured::NothingLive);
    }

    #[test]
    fn a_live_credential_this_switch_would_write_is_not_saved_anywhere() {
        let host = a_home();
        let registry = two_accounts();
        let incoming = credential_json("incoming", Some(1_000));
        write_live(&host, &incoming);
        let prepared = prepared_to_write(&host, &incoming);

        let captured = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            Some(&one_of(&registry, OUTGOING)),
            &profiles(&host, &registry),
        )
        .expect("an interrupted Switch's trace is not a failure");

        assert_eq!(captured, Captured::NothingToSave);
    }

    #[test]
    fn a_repair_that_finds_a_stranger_credential_live_is_refused() {
        let host = a_home();
        let registry = two_accounts();
        write_live(&host, &credential_json("stranger", Some(1_000)));
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));
        let on = one_of(&registry, OUTGOING);

        let refused = capture(
            &host,
            &prepared,
            &on,
            Some(&on),
            &profiles(&host, &registry),
        )
        .expect_err("two readings and nothing tells them apart");

        assert!(matches!(refused, PerchError::Conflict(_)), "{refused}");
    }

    #[test]
    fn an_identity_naming_a_login_perch_does_not_hold_leaves_the_credential_where_it_lies() {
        let host = a_home();
        let registry = two_accounts();
        write_live(&host, &credential_json("live", Some(1_000)));
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));
        host.set_file(
            prepared.store.identity_file.clone(),
            "{\"oauthAccount\":{\"emailAddress\":\"stranger@example.com\"}}",
        );

        let captured = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            Some(&one_of(&registry, OUTGOING)),
            &profiles(&host, &registry),
        )
        .expect("somebody else's Credential is not a failure");

        assert_eq!(
            captured,
            Captured::NotTheirs {
                outgoing: OUTGOING.to_string(),
                live: "stranger@example.com".to_string(),
            }
        );
    }

    #[test]
    fn an_outgoing_copy_newer_than_the_live_one_is_kept() {
        let host = a_home();
        let registry = two_accounts();
        write_live(&host, &credential_json("live", Some(1_000)));
        write_own(
            &host,
            &registry,
            OUTGOING,
            &credential_json("rotated", Some(2_000)),
        );
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));

        let captured = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            Some(&one_of(&registry, OUTGOING)),
            &profiles(&host, &registry),
        )
        .expect("declining is not a failure");

        assert_eq!(
            captured,
            Captured::Superseded {
                outgoing: OUTGOING.to_string(),
            }
        );
    }

    #[test]
    fn a_rotation_is_copied_back_into_the_profile_it_belongs_to() {
        let host = a_home();
        let registry = two_accounts();
        let live = credential_json("live", Some(2_000));
        write_live(&host, &live);
        write_own(
            &host,
            &registry,
            OUTGOING,
            &credential_json("stale", Some(1_000)),
        );
        let prepared = prepared_to_write(&host, &credential_json("incoming", Some(1_000)));

        let captured = capture(
            &host,
            &prepared,
            &one_of(&registry, INCOMING),
            Some(&one_of(&registry, OUTGOING)),
            &profiles(&host, &registry),
        )
        .expect("the ordinary Capture");

        assert_eq!(
            captured,
            Captured::Copied {
                from: OUTGOING.to_string(),
            }
        );
        let store = one_of(&registry, OUTGOING)
            .store(&host)
            .expect("home is known");
        let held = credentials::read(&host, &store)
            .expect("the store answers")
            .expect("it holds the copy now");
        assert_eq!(*held.credential, live, "the live Credential went home");
    }

    #[test]
    fn a_second_opinion_is_read_in_the_order_that_settles_it_most_cheaply() {
        let host = a_home();
        let registry = two_accounts();
        let live = credential_json("live", Some(1_000));
        let outgoing = one_of(&registry, OUTGOING);

        write_own(&host, &registry, OUTGOING, &live);
        assert!(
            matches!(
                corroborates(
                    &host,
                    &profiles(&host, &registry),
                    &outgoing,
                    &one_of(&registry, INCOMING).identity,
                    &live
                ),
                Corroboration::NothingAtStake
            ),
            "the outgoing Profile already holds exactly what is live"
        );

        write_own(
            &host,
            &registry,
            OUTGOING,
            &credential_json("other", Some(1_000)),
        );
        assert!(
            matches!(
                corroborates(
                    &host,
                    &profiles(&host, &registry),
                    &outgoing,
                    &Identity {
                        email: "stranger@example.com".into(),
                        account_uuid: None,
                        organization_uuid: None,
                        organization_name: None
                    },
                    &live
                ),
                Corroboration::NotOurs
            ),
            "an address Perch does not hold corroborates the Identity"
        );

        write_own(
            &host,
            &registry,
            INCOMING,
            &credential_json("different", Some(1_000)),
        );
        assert!(
            matches!(
                corroborates(
                    &host,
                    &profiles(&host, &registry),
                    &outgoing,
                    &one_of(&registry, INCOMING).identity,
                    &live
                ),
                Corroboration::Unaccounted
            ),
            "an Account whose own copy is not the live one corroborates nothing"
        );
    }
}
