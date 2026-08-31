//! `perch add` — a second Account, without costing the session you are in.
//!
//! The login runs in a config directory of its own and the Credential it
//! produces is moved into a Profile afterwards
//! (ADR a-login-perch-does-not-need). Which Account it turned out to be is read
//! back from the login rather than asked for.
//!
//! Nothing reaches the registry until the login has produced an Account Perch
//! can name.

use std::io::Write;

use crate::adopt;
use crate::ask;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::login::{self, Produced};
use crate::name;
use crate::name::NO_GROUP;
use crate::probe::Identity;
use crate::profile;
use crate::registry::{self, Account, Registry};
use crate::say;

#[derive(Debug, Default, Clone, clap::Args)]
pub struct AddArgs {
    /// The Group to put the new Account in. Without it, the Account's
    /// organization is offered as a default for you to confirm.
    #[arg(long, value_name = "NAME")]
    pub group: Option<String>,

    /// Put the new Account in no Group, and ask nothing.
    #[arg(long, conflicts_with = "group")]
    pub no_group: bool,

    /// A short name to reach the Account by, instead of its email address.
    #[arg(long, value_name = "NAME")]
    pub alias: Option<String>,
}

pub fn run(host: &dyn Host, args: AddArgs, out: &mut dyn Write) -> Result<()> {
    // Read rather than held: holding the registry lock across a browser round
    // trip would block every other Perch for as long as the login takes.
    let registry = adopt::ensure_adopted(host)?;

    // Everything knowable before the login is checked before the login, so a
    // name Perch was always going to refuse never costs a browser round trip.
    registry.refuse(registry::Claim::Adding {
        alias: args.alias.as_deref(),
        group: args.group.as_deref(),
    })?;
    if args.group.is_none() && !args.no_group && !host.is_interactive() {
        return Err(PerchError::Invalid(
            "There is no terminal to confirm the Group on. Pass `--group <name>` \
             or `--no-group`."
                .to_string(),
        ));
    }

    let pending = login::perform(host, out, &announcement(&registry))?;
    refuse_an_account_perch_already_holds(host, &registry, &pending.identity)?;
    let group = resolve_group(host, out, &registry, &args, &pending.identity)?;
    drop(registry);

    // Decided against the registry as it is *now*: the copy above was read
    // before a login that may have taken minutes, and writing it back would
    // revert whatever ran meanwhile (ADR a-switch-is-written-down-first).
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
    refuse_an_account_perch_already_holds(host, &registry, &pending.identity)?;
    registry.refuse(registry::Claim::Adding {
        alias: args.alias.as_deref(),
        group: group.as_deref(),
    })?;

    // Naming a Group on `add` declares it, so an Account is never in a Group
    // `perch group list` cannot show. The declared spelling is what is recorded,
    // which puts the Account in the Group that exists rather than beside it.
    let group = match &group {
        Some(name) => Some(registry.ensure_group(name)?),
        None => None,
    };

    let (account, placed) = settle_into_a_profile(host, pending, group.clone())?;
    let email = account.email().to_string();

    // A Profile nothing records is worse than none: it holds a live refresh
    // token that `reap_abandoned` never walks, since that only walks `pending/`.
    // Every step from here to the save is inside the undo, not the save alone.
    let recorded = (|registry: &mut Registry| {
        registry.upsert(account);
        if let Some(alias) = &args.alias {
            // Refused before the login and again here. Nothing has changed
            // under the lock this holds, so it cannot fail.
            registry.name_account(alias, &email)?;
        }
        registry::save(host, &mut perch, registry)
    })(&mut registry);

    if let Err(error) = recorded {
        placed.take_back(host);
        return Err(error.with_note(&format!(
            "Nothing was added, and the Profile this had made for {email} has \
             been taken back out again: a Credential Perch holds and does not \
             record is one nothing would ever look at or delete.\n\
             The login itself worked, so `perch add` will need running again."
        )));
    }

    // On disk by here, so an unnoted failure sends a script back to log in
    // again as an Account Perch already holds.
    report(
        out,
        &registry,
        &email,
        args.alias.as_deref(),
        group.as_deref(),
    )
    .map_err(|error| {
        error.with_note(&format!(
            "The Account was added: {email} has a Profile of its own and Perch \
             holds its Credential. Only the report could not be printed.",
        ))
    })
}

/// Gives the Account a Profile of its own and returns the entry that records
/// it, alongside the ledger the caller needs precisely in order to undo this.
fn settle_into_a_profile(
    host: &dyn Host,
    pending: Produced,
    group: Option<String>,
) -> Result<(Account, profile::Placed)> {
    let dir = holdings::profile_dir_for(host, &pending.identity.email)?;
    // The file the login wrote is already exactly what belongs in the Account's
    // own directory.
    let placed = profile::place(
        host,
        &dir,
        Some(pending.credential.as_str()),
        Some(&pending.identity_json),
        profile::IfItFails::TakeBack,
    )?;

    Ok((
        Account {
            identity: pending.identity,
            plan: pending.credential.subscription_type.clone(),
            disabled: false,
            quarantine: None,
            group,
            utilization: None,
        },
        placed,
    ))
}

/// Refuses a login whose Credential would land in a Profile Perch already holds
/// one in.
///
/// The question is which *Profile*, not which address: two addresses that
/// flatten to one slug are one Profile (ADR claude-code-chooses-the-store).
fn refuse_an_account_perch_already_holds(
    host: &dyn Host,
    registry: &Registry,
    identity: &Identity,
) -> Result<()> {
    let Some(existing) = registry
        .accounts
        .iter()
        .find(|held| holdings::same_profile(held.email(), &identity.email))
    else {
        return Ok(());
    };

    // Over the whole of Unicode, because the collision that got here was:
    // `same_profile` compares slugs and `slug` lowercases first, so an ASCII
    // comparison would make one Profile look like two Accounts.
    let same_account = name::same_name(existing.email(), &identity.email);
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
             that Account first, or log in under an address that does not \
             flatten to the same name.",
            existing.email(),
        )
    };

    Err(PerchError::Conflict(format!(
        "Perch already holds {}, in {}.\n\
         Nothing was added: {why}.\n\
         {way_out}",
        registry.named_for_the_user(existing.email()),
        existing.profile_dir(host)?.display(),
    )))
}

/// Which Group the new Account joins.
///
/// The organization is offered and never assumed: three subscriptions bought
/// personally each carry their own organization, so inferring from it would
/// split exactly the case Groups exist to serve (ADR a-group-is-a-declaration).
fn resolve_group(
    host: &dyn Host,
    out: &mut dyn Write,
    registry: &Registry,
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
    // would go on to refuse is no help as a default, and accepting the offer
    // would re-ask the same question for ever.
    let offered = identity
        .organization_name
        .as_deref()
        .and_then(name::offerable_name)
        .filter(|name| {
            registry
                .refuse(registry::Claim::Adding {
                    alias: args.alias.as_deref(),
                    group: Some(name),
                })
                .is_ok()
        });

    let question = match &offered {
        Some(organization) => format!(
            "Group for {} [{organization}] (Enter to accept, `{NO_GROUP}` for no Group): ",
            identity.email
        ),
        None => format!("Group for {} (Enter for no Group): ", identity.email),
    };

    // A name Perch cannot accept is asked about again rather than failing the
    // command: the login has already happened, and losing the Account over a
    // typo would be a poor trade. Every reason a name can be refused is asked.
    loop {
        let answer = match ask::line(host, out, &question)? {
            Some(answer) => answer.trim().to_string(),
            // End of input after a login that worked, for the same reason.
            None => {
                say::line(out, "\nNo answer given, so the Account is in no Group.")?;
                return Ok(None);
            }
        };

        let chosen = match answer.as_str() {
            "" => offered.clone(),
            named if name::means_the_ungrouped_scope(named) => None,
            named => Some(named.to_string()),
        };

        match &chosen {
            None => return Ok(None),
            Some(name) => match registry.refuse(registry::Claim::Adding {
                alias: args.alias.as_deref(),
                group: Some(name),
            }) {
                Ok(()) => return Ok(chosen),
                Err(err) => say::line(out, &format!("{err}"))?,
            },
        }
    }
}

/// What the login is for, and the one thing somebody mid-session needs to hear
/// before a browser opens: their Account is not the one being logged out.
///
/// Said here rather than again in the report, because this is the moment it is
/// load-bearing (ADR perch-says-what-it-did).
fn announcement(registry: &Registry) -> String {
    format!(
        "Logging in to a new Profile.{}",
        login::leaving_the_active_account_alone(registry.active().whose())
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
    let description = say::described(
        email,
        added.identity.organization_name.as_deref(),
        added.plan.as_deref(),
    );

    say::line(out, &format!("\nAdded {description}."))?;
    if let Some(alias) = alias {
        say::line(out, &format!("Alias:  {alias}"))?;
    }
    let group = group.unwrap_or(name::NO_GROUP);
    say::line(out, &format!("Group:  {group}"))?;
    // What the Scope this Account landed in still cannot do. An Add is what
    // makes a Scope a set of two or more, which is when the two defaults gating
    // a Cycle start to matter.
    match crate::config::what_the_scope_still_needs(registry, &registry.scope_of(added)) {
        Some(line) => say::line(out, &line),
        None => Ok(()),
    }
}
