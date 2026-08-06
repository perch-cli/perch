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

use crate::adopt;
use crate::commands::{ask, say};
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::login::{self, Produced};
use crate::probe::Identity;
use crate::profile;
use crate::registry::{self, Account, NO_GROUP, NameKind, Registry};

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
    // Read rather than held. A login is a browser round trip the user drives,
    // and holding the registry lock across it would block every other Perch on
    // the machine for as long as somebody takes to find their password.
    let registry = adopt::ensure_adopted(host, out)?;

    // Everything knowable before the login is checked before the login, so a
    // name Perch was always going to refuse never costs a browser round trip.
    registry.refuse_taken_names(args.alias.as_deref(), args.group.as_deref())?;
    if let Some(alias) = &args.alias {
        registry::validate_name(NameKind::Alias, alias)?;
    }
    if let Some(group) = &args.group {
        registry::validate_name(NameKind::Group, group)?;
    }
    if args.group.is_none() && !args.no_group && !host.is_interactive() {
        return Err(PerchError::Other(
            "There is no terminal to confirm the Group on. Pass `--group <name>` \
             or `--no-group`."
                .to_string(),
        ));
    }

    let pending = login::perform(host, out, &announcement(&registry))?;
    refuse_an_account_perch_already_holds(host, &registry, &pending.identity)?;
    let group = resolve_group(host, out, &args, &pending.identity)?;
    drop(registry);

    // Everything from here is decided against the registry as it is *now*, with
    // the other Perches shut out. The copy above was read before a login that
    // may have taken minutes, and writing it back would revert whatever else
    // ran in the meantime — a `perch switch` in another terminal, most
    // damagingly, whose `active` the next Capture depends on (ADR 0006).
    let (_perch, mut registry) = adopt::ensure_adopted_exclusively(host, out)?;
    refuse_an_account_perch_already_holds(host, &registry, &pending.identity)?;
    registry.refuse_taken_names(args.alias.as_deref(), group.as_deref())?;

    // Naming a Group on `add` declares it, so an Account is never in a Group
    // that carries no configuration and that `perch group list` cannot show.
    // The Account records the declared spelling, which is what puts it in the
    // Group that already exists rather than beside it.
    let group = match &group {
        Some(name) => Some(registry.ensure_group(name)?),
        None => None,
    };

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

/// Gives the Account a Profile of its own and returns the entry that records
/// it. A Profile that cannot be completed is discarded rather than left
/// half-built for the next command to trip over.
fn settle_into_a_profile(
    host: &dyn Host,
    pending: Produced,
    group: Option<String>,
) -> Result<Account> {
    let dir = registry::profile_dir_for(host, &pending.identity.email)?;
    let store = profile::create(host, &dir, pending.credential.as_str())?;

    // The Identity travels with the Credential it describes: this directory is
    // the Account's own configuration, and the file the login wrote for it is
    // already exactly what belongs there.
    if let Err(err) = login::carry_identity_file(host, &pending.identity_json, &store) {
        profile::discard(host, &store);
        return Err(err);
    }

    Ok(Account {
        identity: pending.identity,
        plan: pending.credential.subscription_type.clone(),
        enabled: true,
        quarantine: None,
        group,
        utilization: None,
    })
}

/// Refuses a login whose Credential would land in a Profile Perch already
/// holds one in.
///
/// Two Profiles for one Account would fight over it — each holding a refresh
/// token the other's next Renewal retires — so the way back to a working
/// Account Perch already has is `perch relogin`, which repairs the Profile
/// rather than building a second one.
///
/// The question is which *Profile* the login would land in, not which email it
/// belongs to. A Profile is `profiles/<slugged email>`, and the slug lowercases
/// and flattens every non-alphanumeric character, so `user+work@gmail.com`,
/// `user.work@gmail.com` and `User-Work@gmail.com` all name one directory and
/// one keychain namespace. Plus-addressing on a single inbox is exactly how
/// somebody comes to hold several Anthropic Accounts, so this is a collision
/// ordinary use produces — and storing over it would supersede the Credential
/// already there and destroy a refresh token nothing can recover.
fn refuse_an_account_perch_already_holds(
    host: &dyn Host,
    registry: &Registry,
    identity: &Identity,
) -> Result<()> {
    let Some(existing) = registry
        .accounts
        .iter()
        .find(|held| registry::same_profile(held.email(), &identity.email))
    else {
        return Ok(());
    };

    let same_account = existing.email().eq_ignore_ascii_case(&identity.email);
    let why = if same_account {
        "two Profiles for one Account would fight over it".to_string()
    } else {
        format!(
            "{} and {} share the Profile they would be kept in, so holding both \
             would mean each one's Credential replacing the other's",
            existing.email(),
            identity.email,
        )
    };
    let way_out = if same_account {
        format!(
            "To repair that Account instead, run `perch relogin {}`.",
            existing.email()
        )
    } else {
        format!(
            "Nothing about {} is changed. To hold this login instead, remove \
             that Account first — or log in under an address that does not \
             flatten to the same name.",
            existing.email(),
        )
    };

    Err(PerchError::Conflict(format!(
        "Perch already holds {}, in {}.\n\
         Nothing was added — {why}.\n\
         {way_out}",
        registry.named_for_the_user(existing.email()),
        existing.profile_dir(host)?.display(),
    )))
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
        .filter(|organization| registry::validate_name(NameKind::Group, organization).is_ok())
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
            Some(name) => match registry::validate_name(NameKind::Group, name) {
                Ok(()) => return Ok(chosen),
                Err(err) => say(out, &format!("{err}"))?,
            },
        }
    }
}

fn announcement(registry: &Registry) -> String {
    format!(
        "Logging in to a new Profile.{}",
        login::leaving_the_active_account_alone(
            registry
                .active_account()
                .map(crate::registry::Account::email)
        )
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
