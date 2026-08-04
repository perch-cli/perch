//! `perch add` — a second Account, without costing you the session you are in.
//!
//! Every Profile after the first is created by launching a login inside it
//! (ADR 0009). The active Account is never read, never written, and never
//! logged out: the login happens in a config directory of its own, and the
//! Credential it produces is moved into a Profile afterwards. Which Account it
//! turned out to be is read back from the login rather than asked for.
//!
//! Nothing reaches the registry until the login has produced an Account Perch
//! can name, so an abandoned login costs a directory that is then removed and
//! nothing else.

use std::io::Write;
use std::path::Path;

use crate::adopt;
use crate::commands::{say, write_failed};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::probe::{self, Credential, Identity};
use crate::profile;
use crate::registry::{self, Account, NO_GROUP, Registry};

#[derive(Debug, Default, Clone)]
pub struct AddArgs {
    /// The Group to put the new Account in. Without it, the Account's
    /// organization is offered as a default for confirmation.
    pub group: Option<String>,
    /// Leave the Account in no Group at all, and ask nothing.
    pub no_group: bool,
    pub alias: Option<String>,
}

pub fn run(host: &dyn Host, args: AddArgs, out: &mut dyn Write) -> Result<()> {
    let mut registry = adopt::ensure_adopted(host, out)?;
    let version = probe::claude_version(host)?;

    // Everything knowable before the login is checked before the login, so a
    // name Perch was always going to refuse never costs a browser round trip.
    registry.refuse_taken_names(args.alias.as_deref(), args.group.as_deref())?;
    if let Some(group) = &args.group {
        registry::validate_group_name(group)?;
    }
    if args.group.is_none() && !args.no_group && !host.is_interactive() {
        return Err(PerchError::Other(
            "There is no terminal to confirm the Group on. Pass `--group <name>` \
             or `--no-group`."
                .to_string(),
        ));
    }

    let pending = login_in_a_directory_of_its_own(host, out, &registry, &version)?;
    let group = resolve_group(host, out, &args, &pending.identity)?;
    registry.refuse_taken_names(args.alias.as_deref(), group.as_deref())?;

    // Naming a Group on `add` declares it, so an Account is never in a Group
    // that carries no configuration and that `perch group list` cannot show.
    if let Some(group) = &group {
        registry.ensure_group(group)?;
    }

    let account = settle_into_a_profile(host, pending, group.clone())?;
    let email = account.email().to_string();
    registry.upsert(account);
    if let Some(alias) = &args.alias {
        registry.set_alias(alias, &email);
    }
    registry::save(host, &registry)?;

    report(
        out,
        &registry,
        &email,
        args.alias.as_deref(),
        group.as_deref(),
    )
}

/// An Account a login has produced but that Perch does not hold yet: taken out
/// of the directory the login ran in, not yet settled into a Profile.
struct PendingAccount {
    identity: Identity,
    credential: Credential,
    /// The `.claude.json` the login wrote, kept verbatim.
    identity_json: String,
}

/// Gives the Account a Profile of its own and returns the entry that records
/// it. A Profile that cannot be completed is discarded rather than left
/// half-built for the next command to trip over.
fn settle_into_a_profile(
    host: &dyn Host,
    pending: PendingAccount,
    group: Option<String>,
) -> Result<Account> {
    let dir = registry::profile_dir_for(host, &pending.identity.email);
    let profile = profile::create(host, &dir, pending.credential.as_str())?;

    // The Identity travels with the Credential it describes: this directory is
    // the Account's own configuration, and the file the login wrote for it is
    // already exactly what belongs there.
    if let Err(err) = carry_identity_file(host, &pending.identity_json, &dir) {
        profile::discard(host, &profile);
        return Err(err);
    }

    Ok(Account {
        identity: pending.identity,
        plan: pending.credential.subscription_type.clone(),
        profile,
        enabled: true,
        quarantined: false,
        group,
        utilization: None,
    })
}

/// Runs the login somewhere the active Account cannot be reached from, and
/// takes what it produced. The directory it ran in is removed either way.
fn login_in_a_directory_of_its_own(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
    version: &str,
) -> Result<PendingAccount> {
    let dir = registry::pending_login_dir(host, host.now());
    host.create_dir_all(&dir)
        .map_err(|err| PerchError::Other(format!("could not create {}: {err}", dir.display())))?;

    let store = probe::store_for_profile(host, &dir)?;
    let ran_in = registry::Profile {
        dir: dir.clone(),
        keychain_service: store.keychain_service.clone(),
        keychain_account: store.keychain_account.clone(),
    };

    announce(out, registry)?;
    let status = host
        .exec_interactive("claude", &[("CLAUDE_CONFIG_DIR", &dir.to_string_lossy())])
        .map_err(|err| PerchError::Other(format!("could not launch a login: {err}")))?;

    let produced = account_the_login_produced(host, &store, version, status, registry);
    profile::discard(host, &ran_in);
    produced
}

/// Reads the Account the login produced, refusing anything Perch cannot record
/// as a new Account — including one it already holds.
fn account_the_login_produced(
    host: &dyn Host,
    store: &probe::Store,
    version: &str,
    status: i32,
    registry: &Registry,
) -> Result<PendingAccount> {
    let credential = probe::read_credential(host, store, version)?;
    let identity = probe::read_identity(host, store, version)?;

    let (credential, identity) = match (credential, identity) {
        (Some(credential), Some(identity)) => (credential, identity),
        // A login that produced neither is one that was abandoned or refused,
        // and the exit status is the only extra thing worth saying about it.
        _ => {
            let ending = if status == 0 {
                "The login did not complete".to_string()
            } else {
                format!("The login exited {status}")
            };
            return Err(PerchError::NotFound(format!(
                "{ending}, so no Account was added. Nothing changed."
            )));
        }
    };

    if let Some(existing) = registry.account(&identity.email) {
        let known_as = registry.named_for_the_user(existing.email());
        return Err(PerchError::Conflict(format!(
            "Perch already holds {known_as}, in {}.\n\
             Nothing was added — two Profiles for one Account would fight over it.\n\
             To repair that Account instead, run `perch relogin {}`.",
            existing.profile.dir.display(),
            existing.email()
        )));
    }

    let identity_json = host.read_file(&store.identity_file).map_err(|err| {
        PerchError::Other(format!(
            "could not read {}: {err}",
            store.identity_file.display()
        ))
    })?;

    Ok(PendingAccount {
        identity,
        credential,
        identity_json,
    })
}

fn carry_identity_file(host: &dyn Host, contents: &str, dir: &Path) -> Result<()> {
    let store = probe::store_for_profile(host, dir)?;
    host.write_file(&store.identity_file, contents)
        .map_err(|err| PerchError::FileWrite {
            path: store.identity_file,
            source: std::io::Error::other(err.to_string()),
        })
}

/// Which Group the new Account joins.
///
/// The organization is offered and never assumed: three subscriptions bought
/// personally each carry their own organization, so inferring from it would
/// split exactly the case Groups exist to serve (ADR 0002).
fn resolve_group(
    host: &dyn Host,
    out: &mut dyn Write,
    args: &AddArgs,
    identity: &Identity,
) -> Result<Option<String>> {
    if args.no_group {
        return Ok(None);
    }
    if let Some(group) = &args.group {
        return Ok(Some(group.clone()));
    }

    // Only offered when it would be a usable Group name: an organization Perch
    // would go on to refuse is no help as a default.
    let offered = identity
        .organization_name
        .as_deref()
        .map(str::trim)
        .filter(|organization| registry::validate_group_name(organization).is_ok())
        .map(str::to_string);

    let question = match &offered {
        Some(organization) => format!(
            "Group for {} [{organization}] (Enter to accept, `{NO_GROUP}` for no Group): ",
            identity.email
        ),
        None => format!("Group for {} (Enter for no Group): ", identity.email),
    };

    // A name Perch cannot accept is asked about again rather than failing the
    // command: the login has already happened by now, and losing the Account
    // over a typo would be a poor trade.
    loop {
        let answer = match ask(host, out, &question)? {
            Some(answer) => answer.trim().to_string(),
            // End of input after a login that worked, for the same reason.
            None => {
                say(out, "\nNo answer given, so the Account is in no Group.")?;
                return Ok(None);
            }
        };

        let chosen = match answer.as_str() {
            "" => offered.clone(),
            named if registry::means_no_group(named) => None,
            named => Some(named.to_string()),
        };

        match &chosen {
            None => return Ok(None),
            Some(name) => match registry::validate_group_name(name) {
                Ok(()) => return Ok(chosen),
                Err(err) => say(out, &format!("{err}"))?,
            },
        }
    }
}

fn announce(out: &mut dyn Write, registry: &Registry) -> Result<()> {
    match registry.active_account() {
        Some(active) => say(
            out,
            &format!(
                "Logging in to a new Profile. {} stays active and its session is untouched.",
                active.email()
            ),
        )?,
        None => say(out, "Logging in to a new Profile.")?,
    }
    say(
        out,
        "Quit Claude Code when the login is done to come back here.\n",
    )
}

fn report(
    out: &mut dyn Write,
    registry: &Registry,
    email: &str,
    alias: Option<&str>,
    group: Option<&str>,
) -> Result<()> {
    let added = registry.account(email).expect("the Account was just added");
    let mut description = email.to_string();
    let details: Vec<String> = [added.identity.organization_name.clone(), added.plan.clone()]
        .into_iter()
        .flatten()
        .collect();
    if !details.is_empty() {
        description.push_str(&format!(" ({})", details.join(", ")));
    }

    say(out, &format!("\nAdded {description}."))?;
    if let Some(alias) = alias {
        say(out, &format!("Alias:  {alias}"))?;
    }
    say(out, &format!("Group:  {}", group.unwrap_or("none")))?;
    match registry.active_account() {
        Some(active) => say(
            out,
            &format!(
                "{} is still the active Account — use `perch switch` to move.",
                active.email()
            ),
        ),
        None => Ok(()),
    }
}

/// Puts a question to the person at the terminal and waits for their answer.
fn ask(host: &dyn Host, out: &mut dyn Write, question: &str) -> Result<Option<String>> {
    write!(out, "{question}").map_err(write_failed)?;
    out.flush().map_err(write_failed)?;
    host.read_line()
        .map_err(|err| PerchError::Other(format!("could not read your answer: {err}")))
}
