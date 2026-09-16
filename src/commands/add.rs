//! `perch add` — a second Account, without costing the session you are in.
//!
//! The login runs in a config directory of its own and the Credential it
//! produces is moved into a Profile afterwards
//! (ADR a-login-perch-does-not-need). Which Account it turned out to be is read
//! back from the login rather than asked for.
//!
//! Nothing reaches the Registry until the login has produced an Account Perch
//! can name.

use std::io::Write;

use crate::ask;
use crate::domain::Identity;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::Host;
use crate::name;
use crate::name::NO_GROUP;
use crate::providers::provider::{Authenticated, InstallMode};
use crate::registry::{self, Account, Registry};
use crate::say;

#[derive(Debug, Default, Clone, clap::Args)]
pub struct AddArgs {
    #[command(flatten)]
    pub provider: super::selection::Selection,
    /// The Group for the new Account
    #[arg(long, value_name = "NAME")]
    pub group: Option<String>,

    /// Put the new Account in no Group
    #[arg(long, conflicts_with = "group")]
    pub no_group: bool,

    /// A short name for the Account
    #[arg(long, value_name = "NAME")]
    pub alias: Option<String>,
}

pub fn run(host: &dyn Host, args: AddArgs, out: &mut dyn Write) -> Result<()> {
    // Read rather than held: holding the Registry lock across a browser round
    // trip would block every other Perch for as long as the login takes.
    let provider = args.provider.explicit()?.unwrap_or_default();
    let installation = provider.adapter().configured(host)?.installation(host)?;
    let mut registry = crate::adopt::ensure_adopted(host)?;
    registry.select_provider(provider);

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

    say::line(out, &announcement())?;
    let pending = installation.authenticate(host)?;
    refuse_an_account_perch_already_holds(&registry, provider, &pending)?;
    let group = resolve_group(host, out, &registry, &args, pending.identity())?;
    drop(registry);

    // Decided against the Registry as it is *now*: the copy above was read
    // before a login that may have taken minutes, and writing it back would
    // revert whatever ran meanwhile (ADR a-switch-is-written-down-first).
    let mut perch = holdings::lock(host)?;
    let mut registry = registry::load(host)?.unwrap_or_default();
    refuse_an_account_perch_already_holds(&registry, provider, &pending)?;
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

    let account = Account {
        storage_key: None,
        provider,
        provider_identity: pending.subject().clone(),
        identity: pending.identity().clone(),
        plan: pending.plan().clone(),
        disabled: false,
        quarantine: None,
        group: group.clone(),
        utilization: None,
    };
    let placed =
        provider
            .adapter()
            .install(host, &account.profile(host)?, &pending, InstallMode::New)?;
    let email = account.key().to_string();

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
        if let Err(cleanup) = placed.rollback() {
            return Err(error.with_note(&format!("Rollback incomplete: {cleanup}")));
        }
        return Err(error.with_note(&format!(
            "Nothing was added. The login as {email} worked, so run `perch add` again."
        )));
    }

    placed.commit();

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
            "{email} was added. Only the report could not be printed."
        ))
    })
}

/// Refuses a login whose Credential would land in a Profile Perch already holds
/// one in.
///
/// The question is which *Profile*, not which address: two addresses that
/// flatten to one slug are one Profile (ADR claude-code-chooses-the-store).
fn refuse_an_account_perch_already_holds(
    registry: &Registry,
    provider: crate::providers::provider::Id,
    pending: &Authenticated,
) -> Result<()> {
    let identity = pending.identity();
    let Some(existing) = registry.accounts.iter().find(|held| {
        held.provider() == provider
            && match pending.subject() {
                Some(subject) => held.provider_identity.as_ref() == Some(subject),
                None => holdings::same_profile(held.key(), &identity.email),
            }
    }) else {
        return Ok(());
    };

    // Over the whole of Unicode, because the collision that got here was:
    // `same_profile` compares slugs and `slug` lowercases first, so an ASCII
    // comparison would make one Profile look like two Accounts.
    let same_account =
        pending.subject().is_some() || name::same_name(existing.key(), &identity.email);
    let named = registry.named_for_the_user(existing.key());
    Err(PerchError::Conflict(if same_account {
        format!(
            "Perch already holds {named}. `perch relogin {}` repairs it.",
            existing.key()
        )
    } else {
        format!(
            "Perch already holds {named}, and {} would share its Profile. \
             `perch remove {}` first.",
            identity.email,
            existing.key(),
        )
    }))
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

/// What the login is for.
fn announcement() -> String {
    "Logging in to a new Profile.".to_string()
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
        added.email(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Execution, FakeHost, fake::Effect};

    /// A writer that is not there — the ordinary closed pipe.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the pipe closed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn a_machine_with_claude_code() -> FakeHost {
        FakeHost::new()
            .with_env("PATH", "/usr/bin")
            .with_file("/usr/bin/claude", "")
            .with_exec(
                "/usr/bin/claude",
                &["--version"],
                Execution {
                    status: 0,
                    stdout: "2.1.221 (Claude Code)\n".to_string(),
                    stderr: String::new(),
                },
            )
    }

    /// A closed pipe is the failure that needs no arranging: it lands between
    /// making the directory and discarding it.
    #[test]
    fn a_login_that_cannot_be_announced_never_starts_the_client() {
        let host = a_machine_with_claude_code();

        assert!(
            run(
                &host,
                AddArgs {
                    no_group: true,
                    ..Default::default()
                },
                &mut Closed
            )
            .is_err(),
            "the line before the browser could not be written"
        );

        assert!(
            host.effects()
                .iter()
                .all(|effect| !matches!(effect, Effect::ExecInteractive { .. })),
            "announcement fails before native login starts: {:?}",
            host.effects()
        );
    }
}
