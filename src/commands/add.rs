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
use crate::probe::{Identity, Store};
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
    let registry = adopt::ensure_adopted(host)?;

    // Everything knowable before the login is checked before the login, so a
    // name Perch was always going to refuse never costs a browser round trip.
    // Each name against the whole namespace, in the order the registry decides
    // — shape before collision, which this command is the reason for.
    if let Some(alias) = &args.alias {
        registry.refuse_a_name_nothing_may_answer_to(NameKind::Alias, alias, None)?;
    }
    // Its shape only, and not whether it is free: naming a Group here *joins*
    // one, and `ensure_group` declares it if nobody has. The half of the check
    // that still applies — that it is not an Alias — is the pair check below.
    if let Some(group) = &args.group {
        registry::validate_name(NameKind::Group, group)?;
    }
    // And the pair against each other, which no check of one name can see: a
    // command that sets both at once could otherwise plant the collision the
    // shared namespace exists to prevent.
    registry.refuse_taken_names(args.alias.as_deref(), args.group.as_deref())?;
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

    // Everything from here is decided against the registry as it is *now*, with
    // the other Perches shut out. The copy above was read before a login that
    // may have taken minutes, and writing it back would revert whatever else
    // ran in the meantime — a `perch switch` in another terminal, most
    // damagingly, whose `active` the next Capture depends on (ADR 0006).
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;
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

    // The Store comes back with the Account, so nothing fallible sits between
    // the Credential landing in the Profile and this being able to take it out
    // again.
    let (account, placed) = settle_into_a_profile(host, pending, group.clone())?;
    let email = account.email().to_string();

    // A Profile that nothing records is worse than no Profile at all: it holds
    // a live refresh token, and nothing ever looks at it again. `reap_abandoned`
    // walks the pending logins and never `profiles/`, so this one would sit
    // there for good — the slow accumulation of working logins for Accounts the
    // user believes they never added. The same all-or-nothing an Import makes.
    //
    // Every step from here to the save is inside it, not the save alone. The
    // naming below is argued to be unreachable and the save is not the only
    // thing that can fail; a discard reached by one of the two ways out and not
    // the others is the shape this comment is about, written down and then only
    // half applied.
    let recorded = (|registry: &mut Registry| {
        registry.upsert(account);
        if let Some(alias) = &args.alias {
            // Refused before the login and again here, against the registry as
            // it is now. Nothing has changed under the lock this holds, so this
            // cannot fail — and a name that reached this point unchecked would
            // be one no command could ever free.
            registry.name_account(alias, &email)?;
        }
        registry::save(host, &mut perch, registry)
    })(&mut registry);

    if let Err(error) = recorded {
        profile::discard(host, &placed);
        return Err(error.with_note(&format!(
            "Nothing was added, and the Profile this had made for {email} has \
             been taken back out again: a Credential Perch holds and does not \
             record is one nothing would ever look at or delete.\n\
             The login itself worked, so `perch add` will need running again."
        )));
    }

    report(
        out,
        &registry,
        &email,
        args.alias.as_deref(),
        group.as_deref(),
    )
}

/// Gives the Account a Profile of its own and returns the entry that records
/// it, alongside the Store that Profile keeps its Credential in. A Profile that
/// cannot be completed is discarded rather than left half-built for the next
/// command to trip over.
///
/// The Store is handed back rather than derived again by the caller, because
/// deriving it is fallible and the caller needs it precisely in order to undo
/// this — so asking for it a second time put one more thing that can fail
/// between the Credential landing and anything being able to take it away.
fn settle_into_a_profile(
    host: &dyn Host,
    pending: Produced,
    group: Option<String>,
) -> Result<(Account, Store)> {
    let dir = registry::profile_dir_for(host, &pending.identity.email)?;
    let store = profile::create(host, &dir, pending.credential.as_str())?;

    // The Identity travels with the Credential it describes: this directory is
    // the Account's own configuration, and the file the login wrote for it is
    // already exactly what belongs there.
    if let Err(err) = login::carry_identity_file(host, &pending.identity_json, &store) {
        profile::discard(host, &store);
        return Err(err);
    }

    Ok((
        Account {
            identity: pending.identity,
            plan: pending.credential.subscription_type.clone(),
            disabled: false,
            quarantine: None,
            group,
            utilization: None,
        },
        store,
    ))
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

    // Over the whole of Unicode, because the collision that got us here was
    // decided over the whole of Unicode: `same_profile` compares slugs, and
    // `slug` lowercases before it does. Asked in ASCII, `café@example.com` and
    // `CAFÉ@example.com` were one Profile and two Accounts — so the refusal
    // withheld `perch relogin` and advised logging in "under an address that
    // does not flatten to the same name", which no address satisfies. Same
    // divergence, and same answer, as the one `target` already fixed for names.
    let same_account = registry::same_name(existing.email(), &identity.email);
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
    // would go on to refuse is no help as a default. An organization name is
    // whatever Anthropic holds rather than something chosen to be typed, so its
    // spaces become the separator the names people pick already use — `Overflow
    // Ltd` is offered as `Overflow-Ltd` rather than not offered at all. Only
    // the spaces: Group names are compared case-insensitively, so rewriting how
    // somebody's organization spells itself would buy nothing.
    let offered = identity
        .organization_name
        .as_deref()
        .and_then(registry::offerable_name);

    let question = match &offered {
        Some(organization) => format!(
            "Group for {} [{organization}] (Enter to accept, `{NO_GROUP}` for no Group): ",
            identity.email
        ),
        None => format!("Group for {} (Enter for no Group): ", identity.email),
    };

    // A name Perch cannot accept is asked about again rather than failing the
    // command: the login has already happened by now, and losing the Account
    // over a typo would be a poor trade. Every reason a name can be refused is
    // asked here, not only whether it is a usable shape — a name that collides
    // with an existing Alias is exactly as much a typo as a name with a space
    // in it, and used to abort the command with the browser round trip already
    // spent.
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
            Some(name) => match registry::validate_name(NameKind::Group, name)
                .and_then(|()| registry.refuse_taken_names(args.alias.as_deref(), Some(name)))
            {
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
    say(
        out,
        &format!("Group:  {}", group.unwrap_or(registry::NO_GROUP)),
    )?;
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
