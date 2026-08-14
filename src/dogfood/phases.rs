//! The phases, in the order they are proved.
//!
//! Declared in the library rather than in `tests/dogfood.rs` because two things
//! need the list and only one of them is a test: the suite drives it, and
//! `dogfood-setup` reports how many of it this machine could prove. A list in a
//! test binary is a list a binary cannot link.
//!
//! Each phase says what it needs of a machine *before* it runs, so the Preflight
//! can turn "what can this machine prove" into a figure rather than into a pile
//! of skip lines somebody scrolls past. What it needs is counted against the
//! machine the *Repair* left behind, so an Account nobody could log back in is
//! not one this list is measured as able to prove anything with.
//!
//! ## Adding one
//!
//! - It steers **policy, never figures** (ADR 0037). A Threshold set below where
//!   an Account already sits fires a Cycle immediately; burning an Account down
//!   to reach a branch is what the fake suites are for.
//! - It reads `list --json`, `status --json` and exit codes. There is no `perch
//!   state --json` and there will not be one. Where it has to see what Perch
//!   *did to the machine* — the links a Reconcile made, the expiry on a
//!   Credential — it looks at the machine directly through [`Perch::host`],
//!   which is the Preflight's own reasoning: the thing under test is not the
//!   thing that should be asked whether it worked.
//! - It never unwinds itself. A phase that fails stops, says what is now true,
//!   and says what puts it back. A Setting it steered is not an unwind — putting
//!   that back is one of the phase's own steps, and a phase that stops before it
//!   gets there says so in the commands it prints.
//! - It can tell a fault in Perch from news about something upstream. One that
//!   cannot does not belong here.
//! - It may ask a person for an **act** and never for a **verdict** (ADR 0038).

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Fault, Halt, Needs, Perch, Phase, Preflight, Proof, Setback, read_as};
use crate::error::{
    EXIT_INVALID, EXIT_NO_CANDIDATE, EXIT_NOT_UNDERSTOOD, EXIT_NOTHING_TO_DO, EXIT_OK,
};

/// The suite, whole.
///
/// Cheap and unattended first, state-moving next, and the two that hand the
/// terminal over last — so nobody is asked for a browser at minute two of a run
/// that was going to stop at minute three.
pub const PHASES: &[Phase] = &[
    Phase {
        name: "the listing and the status agree on which Account is active",
        needs: Needs::THE_ACTIVE_ACCOUNT,
        prove: the_listing_and_the_status_agree,
    },
    Phase {
        name: "a refusal is a refusal, and says so in its exit code",
        needs: Needs::NOTHING,
        prove: a_refusal_is_a_refusal,
    },
    Phase {
        name: "a Run reconciles the Profile it launches",
        needs: Needs::AN_ACCOUNT,
        prove: a_run_reconciles_the_profile_it_launches,
    },
    Phase {
        name: "a Renewal is asked of Anthropic and what comes back is filed",
        needs: Needs {
            client: true,
            network: true,
            ..Needs::THE_ACTIVE_ACCOUNT
        },
        prove: a_renewal_is_asked_of_anthropic,
    },
    Phase {
        name: "a Switch moves the machine and leaves what it left usable",
        needs: Needs::TWO_ACCOUNTS,
        prove: a_switch_moves_the_machine,
    },
    Phase {
        name: "a Cycle chooses within the Group",
        needs: Needs::A_PAIR,
        prove: a_cycle_chooses_within_the_group,
    },
    Phase {
        name: "a Check decides and says so in its exit code",
        needs: Needs::A_PAIR,
        prove: a_check_decides_and_says_so_in_its_exit_code,
    },
    Phase {
        name: "the client accepts what Reconcile built",
        needs: Needs {
            client: true,
            attended: true,
            ..Needs::AN_ACCOUNT
        },
        prove: the_client_accepts_what_reconcile_built,
    },
    Phase {
        name: "an Account is added on the machine that was behind",
        needs: Needs {
            client: true,
            attended: true,
            spare_login: true,
            ..Needs::NOTHING
        },
        prove: an_account_is_added_on_the_machine_that_was_behind,
    },
];

// ---- what a phase sees of the machine --------------------------------------

/// One Account as `perch list --json` renders it.
///
/// Parsed into a shape rather than read key by key at each use, because half of
/// what these phases assert is that the document *has* the keys a script would
/// reach for — and a missing one should be one sentence about the listing rather
/// than a `null` five lines later.
struct Listed {
    email: String,
    group: Option<String>,
    quarantined: bool,
    active: bool,
    profile_dir: PathBuf,
}

/// Every Account in the listing, and which one is active.
fn listing(perch: &Perch<'_>) -> std::result::Result<Vec<Listed>, Halt> {
    let document = perch.json(&["list", "--json"])?;
    let accounts = document["accounts"].as_array().ok_or_else(|| {
        Setback::perch("`perch list --json` printed no `accounts` array".to_string())
    })?;

    accounts
        .iter()
        .map(|account| {
            let email = account["email"]
                .as_str()
                .ok_or_else(|| {
                    Setback::perch(format!(
                        "`perch list --json` printed an Account with no address: {account}"
                    ))
                })?
                .to_string();
            let profile_dir = account["profile_dir"].as_str().ok_or_else(|| {
                Setback::perch(format!(
                    "`perch list --json` says nothing about where {email}'s Profile is"
                ))
            })?;
            Ok(Listed {
                email,
                group: account["group"].as_str().map(str::to_string),
                quarantined: !account["quarantined"].is_null(),
                active: account["active"] == serde_json::Value::Bool(true),
                profile_dir: PathBuf::from(profile_dir),
            })
        })
        .collect()
}

/// The Accounts a phase could act on: listed, and not Quarantined.
fn usable(listed: &[Listed]) -> Vec<&Listed> {
    listed
        .iter()
        .filter(|account| !account.quarantined)
        .collect()
}

/// The Account somebody is on, where the Preflight admitted the phase on the
/// grounds that there is one.
///
/// A stop rather than a skip: the gate said there was an active Account moments
/// ago, so its absence now is Perch disagreeing with itself.
fn the_active_one(listed: &[Listed]) -> std::result::Result<&Listed, Halt> {
    listed.iter().find(|account| account.active).ok_or_else(|| {
        Halt::Stopped(Setback::perch(
            "`perch list --json` flags no Account as active, and the Preflight \
                 named one moments ago"
                .to_string(),
        ))
    })
}

/// Runs a command that is expected to work, and says what it exited with when
/// it did not.
///
/// [`read_as`] decides whose fault a non-zero exit is, which is the same reading
/// `perch.json` does — a held registry or a Live Profile is the machine being
/// busy rather than a defect.
fn worked(perch: &Perch<'_>, args: &[&str]) -> std::result::Result<String, Halt> {
    let execution = perch.run(args)?;
    if !execution.succeeded() {
        return Err(Halt::Stopped(read_as(execution.status).because(format!(
            "`perch {}` exited {}: {}",
            args.join(" "),
            execution.status,
            execution.stderr.trim()
        ))));
    }
    Ok(execution.stdout)
}

// ---- 1: two surfaces, one registry -----------------------------------------

/// Two commands, one registry, no network and nothing changed.
///
/// Boring on purpose, and it is the boring part that carries the proof: that
/// `perch` runs as a process at all, that argv reaches it, that exit nought
/// means what it says, that `--json` prints something a script can parse, and
/// that two surfaces reading one registry through two renderers agree about
/// which Account somebody is on.
///
/// Every one of those is unasserted everywhere else, because every other suite
/// links the library and calls `run` directly.
fn the_listing_and_the_status_agree(
    perch: &Perch<'_>,
    _standing: &Preflight,
    _out: &mut dyn Write,
) -> Proof {
    let document = perch.json(&["list", "--json"])?;
    let status = perch.json(&["status", "--json"])?;

    // Read as a script would read them: `perch list --json` names the active
    // Account as a bare address, and `perch status --json` names it inside the
    // Account it is about. The two are named apart deliberately —
    // `active_account` against `active` — so this is also the assertion that
    // they have not quietly been made one shape.
    let listed = document["active_account"].as_str().ok_or_else(|| {
        Setback::perch(format!(
            "`perch list --json` did not name an active Account: {}",
            document["active_account"]
        ))
    })?;
    let said = status["active"]["email"].as_str().ok_or_else(|| {
        Setback::perch(format!(
            "`perch status --json` did not name the Account it is about: {}",
            status["active"]
        ))
    })?;

    if listed != said {
        return Err(Setback::perch(format!(
            "`perch list --json` says {listed} is active and `perch status --json` \
             says {said} is. One registry, read twice, two answers."
        ))
        .into());
    }

    // The same again from the other side of the listing: every Account carries
    // an `active` flag of its own, and exactly one of them may be true. None, or
    // two, is a registry that Cycling would rank from and nobody could read.
    let accounts = document["accounts"].as_array().ok_or_else(|| {
        Setback::perch("`perch list --json` printed no `accounts` array".to_string())
    })?;
    let flagged: Vec<&str> = accounts
        .iter()
        .filter(|account| account["active"] == serde_json::Value::Bool(true))
        .filter_map(|account| account["email"].as_str())
        .collect();

    if flagged != vec![listed] {
        return Err(Setback::perch(format!(
            "`perch list --json` names {listed} as active, and the Accounts it \
             flags as active are {flagged:?}"
        ))
        .into());
    }

    // Nothing here is a Setback for being upstream news: both commands render
    // from cache and never touch the network (ADR 0015), so there is no
    // upstream for this phase to be about.
    Ok(vec![
        format!(
            "`perch list --json` exited 0 and named {}",
            crate::commands::accounts(accounts.len())
        ),
        format!("`perch status --json` exited 0 and is about {said}"),
        format!("both agree {said} is active, and it is the only Account flagged"),
    ])
}

// ---- 2: the refusals ---------------------------------------------------------

/// What Perch says to a command line it will not act on, and what it exits with
/// when it says it.
///
/// The one phase needing nothing at all of a machine, which is what it is for:
/// on a CI runner every other phase skips, and without this one the Dogfood job
/// asserts nothing but the Preflight. It is also the exception ADR 0037's
/// partition allows — the state behind these refusals could be faked, and the
/// *process boundary* could not: argv, a rendered sentence and an exit code go
/// unasserted in every suite that links the library and calls `run` directly.
///
/// Every reading here is machine-independent by construction. The separator
/// refusal is read off the command line before the parser sees it, so it fires
/// on a machine holding nothing exactly as it does on one holding four Accounts.
fn a_refusal_is_a_refusal(perch: &Perch<'_>, _standing: &Preflight, _out: &mut dyn Write) -> Proof {
    let version = perch.run(&["--version"])?;
    if !version.succeeded() || !version.stdout.to_lowercase().contains("perch") {
        return Err(Setback::perch(format!(
            "`perch --version` exited {} and said {:?}",
            version.status,
            version.stdout.trim()
        ))
        .into());
    }

    // A flag that could belong to either program, typed without the `--` that
    // would say which (ADR 0010). Both readings are real, so the answer is
    // neither reading — and the sentence has to name the line that would have
    // worked rather than complain about an unknown argument.
    let without_a_separator = perch.run(&["run", "whoever", "--resume"])?;
    if without_a_separator.status != EXIT_NOT_UNDERSTOOD {
        return Err(Setback::perch(format!(
            "`perch run whoever --resume` exited {} rather than \
             {EXIT_NOT_UNDERSTOOD}, so a flag with no separator was read as \
             something Perch would act on",
            without_a_separator.status
        ))
        .into());
    }
    if !without_a_separator.stderr.contains("--") {
        return Err(Setback::perch(format!(
            "`perch run whoever --resume` was refused without saying where the \
             separator goes: {}",
            without_a_separator.stderr.trim()
        ))
        .into());
    }

    // Not Perch's refusal but the parser's, and asserted for the same reason:
    // an unknown command that exited 0, or that said nothing, is a binary
    // nobody's script could branch on.
    let nonsense = perch.run(&["definitely-not-a-command"])?;
    if nonsense.status == EXIT_OK || nonsense.stderr.trim().is_empty() {
        return Err(Setback::perch(format!(
            "`perch definitely-not-a-command` exited {} and said {:?}",
            nonsense.status,
            nonsense.stderr.trim()
        ))
        .into());
    }

    Ok(vec![
        format!(
            "`perch --version` exited 0 and named itself: {}",
            version.stdout.trim()
        ),
        format!(
            "`perch run whoever --resume` exited {EXIT_NOT_UNDERSTOOD} and said \
             where the separator goes"
        ),
        format!(
            "`perch definitely-not-a-command` exited {} rather than 0",
            nonsense.status
        ),
    ])
}

// ---- 3: what a Run leaves in a Profile --------------------------------------

/// A real Run, over real bytes, with the observer replaced and nothing else.
///
/// `perch run <target> -- <perch> --version` reaches the probe for Claude Code
/// not at all: naming a program after `--` is a Run of that program, in full —
/// Reconcile, then Carry, then a Live Profile claimed, then a launch with
/// `CLAUDE_CONFIG_DIR` pointed at the Profile. So this proves what Reconcile
/// actually did on this operating system, which is where the three of them
/// genuinely differ, without a client and without a minute of quota.
///
/// That is the amendment to ADR 0037 rather than a hole in it. What has been
/// replaced is who is *watching*: Reconcile is the subject and it runs
/// untouched, and since what it does it does to disk, the program it launches
/// can be anything at all.
///
/// Perch is what it launches because Perch is on every machine this suite runs
/// on, by definition, and exits in milliseconds.
fn a_run_reconciles_the_profile_it_launches(
    perch: &Perch<'_>,
    _standing: &Preflight,
    _out: &mut dyn Write,
) -> Proof {
    let listed = listing(perch)?;
    let account = *usable(&listed).first().ok_or_else(|| {
        Halt::Stopped(Setback::perch(
            "`perch list --json` shows no usable Account, and the Preflight \
             counted one moments ago"
                .to_string(),
        ))
    })?;

    let host = perch.host();
    let shared = crate::registry::the_default_profile(host)
        .map_err(|why| {
            Fault::Upstream.because(format!("the Default Profile could not be found: {why}"))
        })?
        .config_dir;
    let crossing = what_should_cross(host, &shared);
    if crossing.is_empty() {
        return Err(Halt::not_here(format!(
            "the Default Profile at {} holds nothing that Reconcile would link, \
             so a Run here would prove nothing about what crosses",
            shared.display()
        )));
    }

    let bin = perch.bin().display().to_string();
    let ran = perch.run(&["run", &account.email, "--", &bin, "--version"])?;
    if !ran.succeeded() {
        return Err(Halt::Stopped(read_as(ran.status).because(format!(
            "`perch run {} -- perch --version` exited {}: {}",
            account.email,
            ran.status,
            ran.stderr.trim()
        ))));
    }

    // Every piece of Shared State the Default Profile holds is now reachable
    // from the Profile that was launched. Read from the filesystem rather than
    // from Perch: this is the one assertion where asking the thing under test
    // whether it worked would be worth nothing.
    for name in &crossing {
        let landed = account.profile_dir.join(name);
        if !host.path_exists(&landed) && host.link_target(&landed).unwrap_or(None).is_none() {
            return Err(Setback::perch(format!(
                "the Default Profile holds `{name}` and {} does not, after a Run \
                 that reconciled it",
                account.profile_dir.display()
            ))
            .leaving(&[format!(
                "{} was launched and exited 0",
                account.profile_dir.display()
            )])
            .into());
        }
    }

    // A directory can only have crossed as a link, which is the half of
    // Reconcile that is different on every operating system: a symlink here, a
    // junction on Windows, and a refusal of its own where a Profile sits on
    // `/mnt/c` under WSL. A hard link to a *file* cannot be told from the file
    // (ADR 0026), so files are asserted as present above and no further.
    let mut linked = Vec::new();
    for name in &crossing {
        let landed = account.profile_dir.join(name);
        if !host.is_file(&shared.join(name))
            && let Ok(Some(target)) = host.link_target(&landed)
        {
            linked.push(format!("`{name}` → {}", target.display()));
        }
    }

    // The Credential is the one thing that must never cross. A Profile holds its
    // own or holds none; a Profile holding the Default Profile's *as a link* is
    // two Accounts sharing one login, which is the failure the denylist exists
    // for.
    let credential = account.profile_dir.join(crate::probe::CREDENTIALS_FILE);
    if let Ok(Some(target)) = host.link_target(&credential) {
        return Err(Setback::perch(format!(
            "{} is a link to {} — the Credential crossed, and it is the one \
             thing that must not",
            credential.display(),
            target.display()
        ))
        .into());
    }

    // The second Run is what proves the first gave the Profile back: a marker
    // outliving the process that wrote it would make the Profile Live for ever,
    // and Reconcile promises to be a no-op over a machine it has already done.
    let again = perch.run(&["run", &account.email, "--", &bin, "--version"])?;
    if !again.succeeded() {
        return Err(Halt::Stopped(read_as(again.status).because(format!(
            "the same Run a second time exited {}: {}. The first Run either left \
             the Profile Live or left it in a state Reconcile could not repair.",
            again.status,
            again.stderr.trim()
        ))));
    }

    Ok(vec![
        format!(
            "`perch run {} -- perch --version` exited 0, twice",
            account.email
        ),
        format!(
            "everything the Default Profile holds is reachable from {}: {}",
            account.profile_dir.display(),
            crossing.join(", ")
        ),
        match linked.is_empty() {
            true => "nothing that crossed was a directory, so nothing had to be linked".to_string(),
            false => format!("linked rather than copied: {}", linked.join(", ")),
        },
        format!(
            "{} is not a link, so the Credential did not cross",
            credential.display()
        ),
    ])
}

/// What Reconcile would carry into a Profile: everything the Default Profile
/// holds except the four entries that belong to the configuration directory
/// itself.
///
/// Read at Run time from the directory rather than from a list, which is
/// Reconcile's own rule — an entry Perch has never heard of still follows you,
/// and a phase carrying its own list would go out of date on Claude Code's
/// schedule rather than on Perch's.
fn what_should_cross(host: &dyn crate::host::Host, shared: &Path) -> Vec<String> {
    let Ok(entries) = host.list_dir(shared) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| entry.file_name()?.to_str().map(str::to_string))
        .filter(|name| !crate::reconcile::HELD_BACK.contains(&name.as_str()))
        .collect()
}

// ---- 4: a Renewal ------------------------------------------------------------

/// The only phase that asks Anthropic for anything, and the only one that cannot
/// decide when it runs.
///
/// A Renewal happens where the access token has run out and nowhere else —
/// `renew_under_the_lock` hands the stored token back untouched while
/// `usable_at` is true. So the gate is the Credential's expiry, which is not in
/// the registry and is therefore not something the Preflight can count cheaply
/// (ADR 0038). It is read here, for the one Account this phase is about, and a
/// token that has not run out is a skip with a reason somebody can act on rather
/// than a pass that renewed nothing.
///
/// Two things it is careful about, both learned from a run that went red on a
/// machine where nothing was wrong.
///
/// The Credential it reads is the one a Renewal *writes*. For the Account
/// somebody is on that is the Default Profile's, not the Account's own — both
/// copies exist and only one is renewed (ADR 0027, and `observe::holding`) — so
/// a phase reading the Account's own Profile gates on an expiry nothing updates
/// and then reports the Renewal it just asked for as unfiled.
///
/// And whether a Renewal was reached at all is read out of the report rather
/// than inferred from the exit code. ADR 0018 has a refresh that could not renew
/// report what it could not read and still exit 0, so an Anthropic 429 inferred
/// from the status alone becomes "This is a fault in Perch" — which is exactly
/// what ADR 0037 says a phase that cannot tell the two apart does.
fn a_renewal_is_asked_of_anthropic(
    perch: &Perch<'_>,
    _standing: &Preflight,
    _out: &mut dyn Write,
) -> Proof {
    let listed = listing(perch)?;
    let account = the_active_one(&listed)?;
    let host = perch.host();

    let installed = crate::probe::Installed::probed(host).map_err(|why| {
        Fault::Upstream.because(format!("Claude Code could not be asked what it is: {why}"))
    })?;
    // The copy a Renewal renews, which for the active Account is the Default
    // Profile's. This phase is about the active Account by construction —
    // `THE_ACTIVE_ACCOUNT` — so there is no second case to write here, and a
    // branch for one that cannot arise is a branch nothing would ever correct.
    let store = crate::registry::the_default_profile(host).map_err(|why| {
        Fault::Upstream.because(format!("the Default Profile's Credential Store: {why}"))
    })?;
    let credential = |when: &str| -> std::result::Result<crate::probe::Credential, Halt> {
        Ok(crate::probe::read_credential(host, &store, &installed)
            .map_err(|why| Fault::Upstream.because(format!("{when}: {why}")))?
            .ok_or_else(|| {
                Fault::Upstream.because(format!(
                    "{when}: the Default Profile holds no Credential, so Claude \
                     Code is logged out here and there is nothing to renew"
                ))
            })?)
    };

    let before = credential("before the Refresh")?;
    if before.usable_at(host.now()) {
        return Err(Halt::not_here(format!(
            "{}'s access token has not run out, so a Refresh would renew nothing \
             — this phase proves what it is for on a machine whose token has \
             gone stale, which is most mornings",
            account.email
        )));
    }
    let was = before.expires_at;

    // The one command in Perch that touches the network, and the one that
    // reaches a Renewal.
    let document = perch.json(&["status", "--refresh", "--json"])?;
    match what_the_refresh_came_to(&document, &account.email)? {
        Renewal::Reached => {}
        Renewal::NotReached(why) => return Err(Halt::not_here(why)),
        Renewal::Lost => {
            return Err(Setback::perch(format!(
                "Anthropic Rotated {}'s refresh token and Perch could not store \
                 what came back, so the one it holds is retired",
                account.email
            ))
            .leaving(&[format!("{} is Quarantined", account.email)])
            .put_back_with(&[crate::registry::how_to_repair(&account.email)])
            .into());
        }
    }

    // Whether the Renewal was filed, asked of the Credential Store rather than
    // of the command that was supposed to write it. A Rotation that Anthropic
    // performed and Perch failed to keep is the one failure in this area that
    // costs a login, and it is invisible from `--json`.
    if credential("reading the Credential back")?.expires_at == was {
        return Err(Setback::perch(format!(
            "Anthropic renewed {}'s access token and the Default Profile holds \
             the same expiry it did before, so what came back was not filed",
            account.email
        ))
        .into());
    }

    // A Renewal may Rotate, and a Rotation retires what every other machine
    // holds — so a Quarantine appearing *here*, on the Account just renewed and
    // after a report that said it was renewed, is Perch failing to keep what it
    // was handed.
    let after = listing(perch)?;
    if after
        .iter()
        .any(|held| held.email == account.email && held.quarantined)
    {
        return Err(Setback::perch(format!(
            "{} was Quarantined by its own Renewal",
            account.email
        ))
        .put_back_with(&[crate::registry::how_to_repair(&account.email)])
        .into());
    }

    Ok(vec![
        format!("{}'s access token had run out", account.email),
        format!(
            "`perch status --refresh --json` reported a Renewal for {}",
            account.email
        ),
        "the Default Profile holds an expiry it did not hold before, so what \
         Anthropic handed back was filed"
            .to_string(),
        format!("{} is not Quarantined by its own Renewal", account.email),
    ])
}

/// What a `perch status --refresh` came to for the one Account it was asked
/// about.
#[derive(Debug, PartialEq, Eq)]
enum Renewal {
    /// A token came back, so there is something to assert about what was filed.
    Reached,
    /// Nothing was renewed, and Perch said why. Named and counted (ADR 0038)
    /// rather than red: Anthropic rate-limiting a refresh is news about an
    /// afternoon, not a defect anybody can go and fix.
    NotReached(String),
    /// Anthropic Rotated and the new token could not be stored — the one failure
    /// in this area that costs a login, and Perch's to answer for.
    Lost,
}

/// Read out of the report rather than inferred from the exit code.
///
/// The inference is invalid by design: ADR 0018 has a refresh that cannot renew
/// degrade to the cached figure and still exit 0, so an exit code alone cannot
/// tell a Renewal that happened from one Anthropic refused. Reading `--json`
/// — which this phase asks for anyway — is what lets the two be told apart, and
/// telling them apart is the whole of what ADR 0037 requires of a phase.
fn what_the_refresh_came_to(
    document: &serde_json::Value,
    email: &str,
) -> std::result::Result<Renewal, Halt> {
    let attempts = document["refresh"]["accounts"].as_array().ok_or_else(|| {
        Setback::perch(
            "`perch status --refresh --json` printed no `refresh.accounts` array".to_string(),
        )
    })?;
    let mine = attempts
        .iter()
        .find(|attempt| {
            attempt["email"]
                .as_str()
                .is_some_and(|named| crate::registry::same_name(named, email))
        })
        .ok_or_else(|| {
            Setback::perch(format!(
                "`perch status --refresh --json` says nothing about {email}, \
                 which is the Account it was asked to refresh"
            ))
        })?;
    let detail = mine["detail"].as_str().unwrap_or("no reason was given");

    match mine["outcome"].as_str() {
        // A token came back in both cases. `throttled` is the Utilization
        // allowance being spent (ADR 0015), and that is met *after* the
        // Renewal: `usable_token` has already renewed by the time the usage
        // endpoint is asked for anything.
        Some("observed" | "throttled") => Ok(Renewal::Reached),
        Some("failed") => Ok(Renewal::NotReached(format!(
            "nothing was renewed for {email}, and Perch said why — so there is \
             no Renewal here to prove: {detail}"
        ))),
        Some("quarantined") if detail == crate::registry::Quarantine::RotationLost.as_str() => {
            Ok(Renewal::Lost)
        }
        // Every other Quarantine is a refresh token something upstream retired
        // — usually this suite, running on another machine yesterday, which ADR
        // 0037 calls the ordinary starting state rather than an ambush.
        Some("quarantined") => Ok(Renewal::NotReached(format!(
            "{email} was Quarantined `{detail}` rather than renewed, which is a \
             login ended somewhere else rather than a fault here. {}",
            crate::registry::how_to_repair(email)
        ))),
        Some(other) => Err(Setback::perch(format!(
            "`perch status --refresh --json` reported `{other}` for {email}, \
             which is not an outcome this phase knows how to read"
        ))
        .into()),
        None => Err(Setback::perch(format!(
            "the refresh report for {email} carries no `outcome`"
        ))
        .into()),
    }
}

// ---- 5: a Switch ------------------------------------------------------------

/// Moving the machine from one Account to another, over real bytes.
///
/// It does not put itself back, and that is the design rather than an omission
/// (ADR 0037): the next phase is measured against the registry as this one left
/// it, and an unwind that failed on a real machine would leave a state nobody
/// could read.
fn a_switch_moves_the_machine(
    perch: &Perch<'_>,
    _standing: &Preflight,
    _out: &mut dyn Write,
) -> Proof {
    let listed = listing(perch)?;
    let leaving = the_active_one(&listed)?;
    let arriving = usable(&listed)
        .into_iter()
        .find(|account| account.email != leaving.email)
        .ok_or_else(|| {
            Halt::Stopped(Setback::perch(
                "there is no second usable Account to Switch to, and the \
                 Preflight counted two moments ago"
                    .to_string(),
            ))
        })?;

    worked(perch, &["switch", &arriving.email])?;

    let after = listing(perch)?;
    let landed = the_active_one(&after)?;
    if landed.email != arriving.email {
        return Err(Setback::perch(format!(
            "`perch switch {}` exited 0 and {} is the Account this machine is on",
            arriving.email, landed.email
        ))
        .leaving(&[format!("the machine is on {}", landed.email)])
        .put_back_with(&[format!("perch switch {}", leaving.email)])
        .into());
    }

    // The Account that was left has to still be one this machine can use. A
    // Capture that did not happen loses whatever Rotation occurred while it was
    // active, and the first evidence of that is a Quarantine on the way back.
    if after
        .iter()
        .any(|held| held.email == leaving.email && held.quarantined)
    {
        return Err(Setback::perch(format!(
            "{} was Quarantined by being Switched away from",
            leaving.email
        ))
        .leaving(&[format!("the machine is on {}", arriving.email)])
        .put_back_with(&[crate::registry::how_to_repair(&leaving.email)])
        .into());
    }

    // Both surfaces, because a Switch writes the Credential one step before it
    // records what it did, and a Landing that is not recorded is the one state
    // this design cannot recover from (ADR 0006).
    let status = perch.json(&["status", "--json"])?;
    let said = status["active"]["email"].as_str().unwrap_or_default();
    if said != arriving.email {
        return Err(Setback::perch(format!(
            "`perch list --json` says the machine is on {} and `perch status \
             --json` says {said}. A Landing that was not recorded reads exactly \
             like this.",
            arriving.email
        ))
        .leaving(&[format!(
            "the machine is on {} by one reading",
            arriving.email
        )])
        .put_back_with(&[format!("perch switch {}", arriving.email)])
        .into());
    }

    Ok(vec![
        format!("this machine was on {}", leaving.email),
        format!("`perch switch {}` exited 0", arriving.email),
        format!("both surfaces now say {} is active", arriving.email),
        format!("{} is still usable, so the Capture kept it", leaving.email),
        format!(
            "nothing was put back: this machine is on {} from here",
            arriving.email
        ),
    ])
}

// ---- 6: a Cycle -------------------------------------------------------------

/// Choosing rather than being told, inside the Group somebody set aside.
///
/// The arranged Group and never the largest one: Cycling moves Accounts around a
/// declaration somebody made, and the only declaration this suite may act on is
/// the one the wizard was told about.
///
/// It steers no policy and reads no figure. Whether a candidate has Headroom is
/// Anthropic's answer on the day, so a Cycle with nowhere to go is news rather
/// than a defect, and this phase skips on it.
fn a_cycle_chooses_within_the_group(
    perch: &Perch<'_>,
    standing: &Preflight,
    _out: &mut dyn Write,
) -> Proof {
    let group = standing.arrangement.group.clone().ok_or_else(|| {
        Halt::Stopped(Setback::perch(
            "no Group is set aside, and the Preflight admitted this phase anyway".to_string(),
        ))
    })?;
    let listed = listing(perch)?;
    let members: Vec<&Listed> = usable(&listed)
        .into_iter()
        .filter(|account| account.group.as_deref() == Some(group.as_str()))
        .collect();
    let [first, ..] = members.as_slice() else {
        return Err(Halt::Stopped(Setback::perch(format!(
            "`perch list --json` shows no usable Account in `{group}`, and the \
             Preflight counted two moments ago"
        ))));
    };

    // Inside the Group before Cycling, because a Cycle happens within the Group
    // of the Account somebody is on — starting anywhere else would move Accounts
    // around a Group nobody set aside.
    let started_on = the_active_one(&listed)?.email.clone();
    if !members.iter().any(|member| member.email == started_on) {
        worked(perch, &["switch", &first.email])?;
    }

    let on_before = the_active_one(&listing(perch)?)?.email.clone();
    let cycled = perch.run(&["switch"])?;
    match cycled.status {
        // Every Account in the Group is full, or on cooldown, or Disabled. All
        // of those are figures and policy on the day rather than a defect, and a
        // phase that burned an Account down to reach the other branch is what
        // ADR 0037 says the fake suites are for.
        EXIT_NO_CANDIDATE => {
            return Err(Halt::not_here(format!(
                "nothing in `{group}` had the Headroom to be Cycled to: {}",
                cycled.stderr.trim()
            )));
        }
        EXIT_OK => {}
        status => {
            return Err(Halt::Stopped(read_as(status).because(format!(
                "`perch switch` with no target exited {status}: {}",
                cycled.stderr.trim()
            ))));
        }
    }

    let landed = listing(perch)?;
    let now_on = the_active_one(&landed)?;
    if now_on.group.as_deref() != Some(group.as_str()) {
        return Err(Setback::perch(format!(
            "a Cycle from {on_before} landed on {}, which is in {} rather than in \
             the Group it was Cycling within",
            now_on.email,
            now_on.group.as_deref().unwrap_or("no Group")
        ))
        .leaving(&[format!("the machine is on {}", now_on.email)])
        .put_back_with(&[format!("perch switch {on_before}")])
        .into());
    }

    Ok(vec![
        format!("this machine was on {on_before}, in `{group}`"),
        "`perch switch` with no target exited 0".to_string(),
        format!(
            "it chose {}, which is in `{group}` — nothing outside the Group was \
             a candidate",
            now_on.email
        ),
    ])
}

// ---- 7: a Check -------------------------------------------------------------

/// One round of the Watcher, run as a scheduler would run it, saying what it
/// decided in its exit code.
///
/// The policy it steers is steered in the safe direction and only ever there:
/// the Group is told the watcher may *not* act, the Check is asked what it makes
/// of that, and the Setting is put back. Nothing here can turn unattended
/// Switching on, which is the one Setting on this machine whose accidental value
/// would act on somebody's Accounts while nobody was watching.
///
/// Putting the Setting back is one of this phase's own steps and not an unwind.
/// A stop before it gets there says what is now true and prints the line that
/// restores it, like every other phase that stops.
fn a_check_decides_and_says_so_in_its_exit_code(
    perch: &Perch<'_>,
    standing: &Preflight,
    _out: &mut dyn Write,
) -> Proof {
    let group = standing.arrangement.group.clone().ok_or_else(|| {
        Halt::Stopped(Setback::perch(
            "no Group is set aside, and the Preflight admitted this phase anyway".to_string(),
        ))
    })?;
    const SETTING: &str = "watcher-may-act";

    let was = read_the_setting(perch, &group, SETTING)?;
    let restore = was.restores(&group, SETTING);
    worked(perch, &["config", "set", &group, SETTING, "false"])?;

    // From here on every failure has a Setting to put back, so the two are said
    // together rather than remembered separately.
    let stopped = |setback: Setback| -> Halt {
        Halt::Stopped(
            setback
                .leaving(&[format!("`{group}` has {SETTING} set to false")])
                .put_back_with(std::slice::from_ref(&restore)),
        )
    };

    let checked = perch.run(&["watch", "--once"]).map_err(&stopped)?;
    if checked.status != EXIT_INVALID {
        return Err(stopped(Setback::perch(format!(
            "`{group}` has been told the watcher may not act on it, and `perch \
             watch --once` exited {} rather than {EXIT_INVALID}: {}",
            checked.status,
            checked.stderr.trim()
        ))));
    }
    if !checked.stderr.contains(SETTING) {
        return Err(stopped(Setback::perch(format!(
            "`perch watch --once` refused without naming the Setting that would \
             change its mind: {}",
            checked.stderr.trim()
        ))));
    }

    // The Setting back where it was, through the binary under test, which is
    // also the assertion that `config unset` and `config set` do what the
    // reading above said they would.
    let put_back = perch
        .run(&restore.split(' ').skip(1).collect::<Vec<_>>())
        .map_err(&stopped)?;
    if !put_back.succeeded() {
        return Err(stopped(Setback::perch(format!(
            "`{restore}` exited {}: {}",
            put_back.status,
            put_back.stderr.trim()
        ))));
    }

    Ok(vec![
        format!("`{group}` was told the watcher may not act on it"),
        format!(
            "`perch watch --once` exited {EXIT_INVALID} and named {SETTING} as \
             what would change its mind"
        ),
        format!("`{restore}` put the Setting back, and exited 0"),
        format!(
            "a Check that decided nothing exits {EXIT_INVALID} rather than \
             {EXIT_NOTHING_TO_DO}, which is what a scheduler branches on"
        ),
    ])
}

/// What a Scope holds for one Setting, and how to put it back.
///
/// `perch config get <scope> <key>` prints the tail of the `set` that would
/// restore it: three words where the Scope holds an Override of its own, and two
/// where it Inherits Global. The difference is the whole of what this reads for
/// — restoring an Inherited Setting with a `set` would leave an Override behind
/// that tracks nothing, which is a different machine from the one the phase
/// found.
enum Held {
    Override(String),
    Inherited,
}

impl Held {
    fn restores(&self, scope: &str, key: &str) -> String {
        match self {
            Held::Override(value) => format!("perch config set {scope} {key} {value}"),
            Held::Inherited => format!("perch config unset {scope} {key}"),
        }
    }
}

fn read_the_setting(perch: &Perch<'_>, scope: &str, key: &str) -> std::result::Result<Held, Halt> {
    let said = worked(perch, &["config", "get", scope, key])?;
    let line = said
        .lines()
        .find(|line| line.contains(key))
        .ok_or_else(|| {
            Setback::perch(format!(
                "`perch config get {scope} {key}` said nothing about {key}: {}",
                said.trim()
            ))
        })?;

    let words: Vec<&str> = line.split_whitespace().collect();
    match words.as_slice() {
        [held, _, value] if *held == scope => Ok(Held::Override((*value).to_string())),
        [_, _] => Ok(Held::Inherited),
        _ => Err(Setback::perch(format!(
            "`perch config get {scope} {key}` printed a line this phase cannot \
             read back: {line}"
        ))
        .into()),
    }
}

// ---- 8: the client ----------------------------------------------------------

/// The one phase that launches a real Claude Code, and the reason it has to
/// exist: whether the client accepts what Reconcile built is a question only the
/// client can answer.
///
/// The person is asked to *quit it*, which is an act. What it came to is judged
/// from the machine afterwards — the exit status, the Profile's own Credential,
/// whether a Quarantine appeared, and whether the Profile was given back — and
/// never from a keystroke (ADR 0038).
fn the_client_accepts_what_reconcile_built(
    perch: &Perch<'_>,
    _standing: &Preflight,
    out: &mut dyn Write,
) -> Proof {
    let listed = listing(perch)?;
    let account = *usable(&listed).first().ok_or_else(|| {
        Halt::Stopped(Setback::perch(
            "`perch list --json` shows no usable Account, and the Preflight \
             counted one moments ago"
                .to_string(),
        ))
    })?;

    writeln!(
        out,
        "  Claude Code is about to start against {}. Check that it came up as \
         that Account, then quit it — `/exit`, or Ctrl-D. Nothing you type is \
         read as a verdict; what this phase asserts is read off the machine \
         after you have quit.",
        account.email
    )
    .map_err(|err| {
        Fault::Upstream.because(format!("the terminal could not be written to: {err}"))
    })?;
    perch.ask(out, "  Return when you are ready. ")?;

    let ended = perch.interactive(&["run", &account.email])?;
    if ended != EXIT_OK {
        // A Run reports what the *program* exited with (ADR 0010), so this is
        // Claude Code's answer rather than Perch's — news, unless Perch refused
        // before the launch, which `read_as` is what tells apart.
        return Err(Halt::Stopped(
            read_as(ended).because(format!("`perch run {}` exited {ended}", account.email)),
        ));
    }

    let host = perch.host();
    // A Profile with no Credential of its own after a client has run in it is a
    // client that logged the Account out, or a Reconcile that let the Credential
    // cross and then had it taken away.
    let store = crate::probe::store_for_profile(host, &account.profile_dir).map_err(|why| {
        Fault::Upstream.because(format!("{}'s Credential Store: {why}", account.email))
    })?;
    let held = crate::credentials::read(host, &store)
        .map_err(|why| Fault::Upstream.because(format!("{}'s Credential: {why}", account.email)))?;
    if held.is_none() {
        return Err(Setback::perch(format!(
            "{} holds no Credential after a client ran against its Profile",
            account.email
        ))
        .leaving(&["a client has been run and has exited".to_string()])
        .put_back_with(&[crate::registry::how_to_repair(&account.email)])
        .into());
    }

    let after = listing(perch)?;
    if after
        .iter()
        .any(|held| held.email == account.email && held.quarantined)
    {
        return Err(Setback::perch(format!(
            "{} was Quarantined by having a client run against it",
            account.email
        ))
        .put_back_with(&[crate::registry::how_to_repair(&account.email)])
        .into());
    }

    // The Profile has to have been given back. A marker outliving the client
    // that wrote it makes the Profile Live for ever, and every Capture, Renewal
    // and Carry against it is refused from then on.
    let bin = perch.bin().display().to_string();
    let again = perch.run(&["run", &account.email, "--", &bin, "--version"])?;
    if !again.succeeded() {
        return Err(Halt::Stopped(read_as(again.status).because(format!(
            "the Profile a client had been running against would not take a \
             second Run: exited {}: {}",
            again.status,
            again.stderr.trim()
        ))));
    }

    Ok(vec![
        format!(
            "Claude Code was launched against {} and exited 0",
            account.email
        ),
        format!(
            "{} holds a Credential of its own in {}",
            account.email,
            account.profile_dir.display()
        ),
        format!("{} is not Quarantined", account.email),
        "the Profile was given back: a second Run against it exited 0".to_string(),
    ])
}

// ---- 9: catching a machine up ------------------------------------------------

/// A real browser login, on the one machine where that is safe: the one holding
/// fewer Accounts than there are logins.
///
/// It keeps what it adds. The machine was behind, and un-adding the Account
/// would put it back where it started — `remove` deletes the Credential, and
/// only a fresh login brings the Account back, as a new one. What `remove` does
/// is provable without a live Credential, so ADR 0037's partition puts it in the
/// deterministic suites rather than here.
fn an_account_is_added_on_the_machine_that_was_behind(
    perch: &Perch<'_>,
    standing: &Preflight,
    out: &mut dyn Write,
) -> Proof {
    let before = listing(perch)?;
    let held: Vec<&str> = before
        .iter()
        .map(|account| account.email.as_str())
        .collect();

    writeln!(
        out,
        "  This machine holds {} and you hold {} in all, so one login is missing \
         from here. `perch add` will open a browser; log in as the Account this \
         machine has not got. The Account you are on now stays active.",
        crate::commands::accounts(held.len()),
        super::logins(standing.arrangement.logins),
    )
    .map_err(|err| {
        Fault::Upstream.because(format!("the terminal could not be written to: {err}"))
    })?;
    if perch.ask(out, "  Walk the login now? [y/N] ")? != "y" {
        return Err(Halt::not_here(
            "the login was not walked, so this machine is still behind".to_string(),
        ));
    }

    let ended = perch.interactive(&["add"])?;
    if ended != EXIT_OK {
        // An abandoned browser login is news about somebody's evening rather
        // than a defect, and `read_as` is deliberately not what reads this: a
        // command that handed the terminal to a person exits non-zero for
        // reasons a phase cannot tell apart from a defect, so the whole of it is
        // reported as upstream and the run carries on.
        return Err(Halt::Stopped(Fault::Upstream.because(format!(
            "`perch add` exited {ended}, so nothing was added"
        ))));
    }

    let after = listing(perch)?;
    let arrived: Vec<&Listed> = after
        .iter()
        .filter(|account| !held.contains(&account.email.as_str()))
        .collect();
    let [added] = arrived.as_slice() else {
        return Err(Setback::perch(format!(
            "`perch add` exited 0 and this machine holds {} rather than the {} it \
             held before: {}",
            crate::commands::accounts(after.len()),
            crate::commands::accounts(held.len()),
            arrived
                .iter()
                .map(|account| account.email.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .into());
    };

    if added.quarantined {
        return Err(Setback::perch(format!(
            "{} arrived Quarantined from its own login",
            added.email
        ))
        .put_back_with(&[crate::registry::how_to_repair(&added.email)])
        .into());
    }

    // A login that added an Account without a Profile to keep it in is one
    // nothing could be Run or Switched to, and the listing would show it anyway.
    let host = perch.host();
    if !host.path_exists(&added.profile_dir) {
        return Err(Setback::perch(format!(
            "{} was added and {} is not there, so the Account has nowhere to keep \
             a Credential",
            added.email,
            added.profile_dir.display()
        ))
        .into());
    }

    // The Account somebody was on has to be the Account they are still on:
    // gaining an Account never costs you the one you are using.
    let was_on = before.iter().find(|account| account.active);
    let now_on = after.iter().find(|account| account.active);
    if let (Some(was), Some(now)) = (was_on, now_on)
        && was.email != now.email
    {
        return Err(Setback::perch(format!(
            "this machine was on {} before the login and is on {} after it",
            was.email, now.email
        ))
        .leaving(&[format!("{} was added and is not active", added.email)])
        .put_back_with(&[format!("perch switch {}", was.email)])
        .into());
    }

    Ok(vec![
        format!(
            "this machine held {} and now holds {}",
            crate::commands::accounts(held.len()),
            crate::commands::accounts(after.len())
        ),
        format!("`perch add` exited 0 and {} arrived", added.email),
        format!(
            "{} is not Quarantined and keeps its Profile at {}",
            added.email,
            added.profile_dir.display()
        ),
        match was_on {
            Some(was) => format!("this machine is still on {}", was.email),
            None => "this machine was on no Account, and still is".to_string(),
        },
        format!(
            "nothing was put back: {} stays, because the machine was behind",
            added.email
        ),
    ])
}

/// A phase only ever runs against a real machine, which is the one place its own
/// reading cannot be checked: a phase that quietly agreed with anything would
/// pass on every machine in the matrix and prove none of them.
///
/// So the reading is asserted here, against canned documents. What is being
/// tested is the phase, not Perch — the documents stand in for what `perch list
/// --json` and `perch status --json` print, and the point is that a disagreement
/// between them is caught rather than passed over.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Execution, FakeHost, Host};

    const BIN: &str = "/build/perch";

    fn said(stdout: &str) -> Execution {
        Execution {
            status: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn printing(document: &str, status: &str) -> FakeHost {
        FakeHost::new()
            .with_exec(BIN, &["list", "--json"], said(document))
            .with_exec(BIN, &["status", "--json"], said(status))
    }

    fn a_listing(active: &str, accounts: &[(&str, bool)]) -> String {
        let listed: Vec<String> = accounts
            .iter()
            .map(|(email, flagged)| format!(r#"{{"email": "{email}", "active": {flagged}}}"#))
            .collect();
        format!(
            r#"{{"active_account": "{active}", "accounts": [{}]}}"#,
            listed.join(", ")
        )
    }

    fn a_status(email: &str) -> String {
        format!(r#"{{"active": {{"email": "{email}"}}}}"#)
    }

    fn proving(host: &dyn Host, phase: &Phase) -> Proof {
        let mut out = Vec::new();
        (phase.prove)(
            &Perch::under_test(host, BIN),
            &Preflight::taken(host).expect("a machine describes itself"),
            &mut out,
        )
    }

    fn agreeing(host: &dyn Host) -> Proof {
        proving(host, &PHASES[0])
    }

    #[test]
    fn two_surfaces_reading_one_registry_and_agreeing_is_what_is_proved() {
        let host = printing(
            &a_listing(
                "one@example.com",
                &[("one@example.com", true), ("two@example.com", false)],
            ),
            &a_status("one@example.com"),
        );

        let proved = agreeing(&host).expect("they agree");

        assert!(proved.iter().any(|line| line.contains("named 2 Accounts")));
        assert!(proved.iter().any(|line| line.contains("one@example.com")));
    }

    #[test]
    fn two_answers_to_which_account_is_active_is_a_fault_in_perch() {
        let host = printing(
            &a_listing("one@example.com", &[("one@example.com", true)]),
            &a_status("two@example.com"),
        );

        let Err(Halt::Stopped(setback)) = agreeing(&host) else {
            panic!("they do not agree");
        };

        assert_eq!(setback.fault, Fault::Perch);
        assert!(setback.because.contains("One registry, read twice"));
    }

    /// None flagged, or two, is the failure the flag exists to make visible: it
    /// is what Cycling ranks from, and a listing that agrees with `status` while
    /// flagging the wrong row is a listing somebody would act on.
    #[test]
    fn a_listing_that_flags_no_account_or_two_is_caught_even_though_the_two_surfaces_agree() {
        for accounts in [
            &[("one@example.com", false)][..],
            &[("one@example.com", true), ("two@example.com", true)][..],
        ] {
            let host = printing(
                &a_listing("one@example.com", accounts),
                &a_status("one@example.com"),
            );

            let Err(Halt::Stopped(setback)) = agreeing(&host) else {
                panic!("the flags do not add up");
            };

            assert!(setback.because.contains("flags as active"), "{setback:?}");
        }
    }

    #[test]
    fn a_command_that_failed_is_reported_as_what_it_exited_with() {
        let host = FakeHost::new().with_exec(
            BIN,
            &["list", "--json"],
            Execution {
                status: crate::error::EXIT_NOT_FOUND,
                stdout: String::new(),
                stderr: "Perch holds no Accounts.".to_string(),
            },
        );

        let Err(Halt::Stopped(setback)) = agreeing(&host) else {
            panic!("nothing to list");
        };

        assert!(setback.because.contains("exited 12"), "{setback:?}");
        assert!(setback.because.contains("Perch holds no Accounts."));
    }

    #[test]
    fn output_that_is_not_json_is_said_to_be_that_rather_than_read_past() {
        let host = printing("not json at all", &a_status("one@example.com"));

        let Err(Halt::Stopped(setback)) = agreeing(&host) else {
            panic!("that is not a document");
        };

        assert!(setback.because.contains("not JSON"), "{setback:?}");
    }

    // ---- the refusals -------------------------------------------------------

    /// What a machine that answers every refusal correctly looks like. The
    /// phase needs nothing of it, which is the point: this is the CI runner.
    fn refusing_properly() -> FakeHost {
        FakeHost::new()
            .with_exec(BIN, &["--version"], said("perch 0.1.1"))
            .with_exec(
                BIN,
                &["run", "whoever", "--resume"],
                Execution {
                    status: EXIT_NOT_UNDERSTOOD,
                    stdout: String::new(),
                    stderr: "`--resume` is Claude Code's. Everything meant for the \
                             program you are running goes after `--`:\n\n    perch \
                             run whoever -- --resume\n"
                        .to_string(),
                },
            )
            .with_exec(
                BIN,
                &["definitely-not-a-command"],
                Execution {
                    status: EXIT_NOT_UNDERSTOOD,
                    stdout: String::new(),
                    stderr: "error: unrecognized subcommand".to_string(),
                },
            )
    }

    #[test]
    fn the_refusals_are_proved_on_a_machine_holding_nothing_at_all() {
        let proved = proving(&refusing_properly(), &PHASES[1]).expect("every refusal is right");

        assert_eq!(proved.len(), 3);
        assert!(proved.iter().any(|line| line.contains("separator goes")));
    }

    #[test]
    fn a_flag_typed_without_the_separator_that_perch_acted_on_is_a_fault() {
        let host = refusing_properly().with_exec(
            BIN,
            &["run", "whoever", "--resume"],
            Execution {
                status: crate::error::EXIT_NOT_FOUND,
                stdout: String::new(),
                stderr: "no Account called whoever".to_string(),
            },
        );

        let Err(Halt::Stopped(setback)) = proving(&host, &PHASES[1]) else {
            panic!("a flag with no separator was read as a Target's");
        };

        assert_eq!(setback.fault, Fault::Perch);
        assert!(setback.because.contains("separator"), "{setback:?}");
    }

    #[test]
    fn an_unknown_command_that_exited_nought_is_a_fault() {
        let host = refusing_properly().with_exec(
            BIN,
            &["definitely-not-a-command"],
            said("nothing to see here"),
        );

        let Err(Halt::Stopped(setback)) = proving(&host, &PHASES[1]) else {
            panic!("an unknown command succeeded");
        };

        assert!(setback.because.contains("definitely-not-a-command"));
    }

    // ---- what a Refresh came to --------------------------------------------

    /// As `observe::Report` writes it, so a change to the shape of that document
    /// arrives here as a failing test rather than as a phase quietly reading
    /// `None` off every key it wanted.
    fn a_refresh(email: &str, outcome: &str, detail: &str) -> serde_json::Value {
        serde_json::json!({
            "refresh": {
                "accounts": [{"email": email, "outcome": outcome, "detail": detail}],
                "kept": true,
            }
        })
    }

    const WHOEVER: &str = "one@example.com";

    /// The reading this phase was rewritten for. A run went red on a machine
    /// where nothing was wrong: Anthropic rate-limited the Renewal, `perch
    /// status --refresh` degraded to the cached figure and exited 0 exactly as
    /// ADR 0018 says it must, and the phase read the exit code alone and called
    /// it a fault in Perch.
    #[test]
    fn a_renewal_anthropic_refused_is_news_rather_than_a_fault() {
        let rate_limited = "Anthropic is rate-limiting Perch, so nothing about \
                            this Account could be read. The cached figure is \
                            what you see.";

        let came_to =
            what_the_refresh_came_to(&a_refresh(WHOEVER, "failed", rate_limited), WHOEVER)
                .expect("a report that says what happened is a report that can be read");

        let Renewal::NotReached(why) = came_to else {
            panic!("a refusal upstream was read as a Renewal: {came_to:?}");
        };
        // The reason Perch gave travels, because the whole point of not being
        // red is that somebody can still see what happened.
        assert!(why.contains("rate-limiting"), "{why}");
    }

    #[test]
    fn a_renewal_that_reached_anthropic_is_something_to_assert_about() {
        for outcome in ["observed", "throttled"] {
            assert_eq!(
                what_the_refresh_came_to(&a_refresh(WHOEVER, outcome, "null"), WHOEVER)
                    .expect("a report that can be read"),
                Renewal::Reached,
                "`{outcome}` is met after `usable_token` has already renewed"
            );
        }
    }

    /// The one Quarantine here that is Perch's: Anthropic Rotated, and what came
    /// back could not be stored. Every other one is a login ended somewhere
    /// else, which ADR 0037 calls the ordinary starting state.
    #[test]
    fn only_a_rotation_perch_could_not_keep_is_a_fault_in_perch() {
        assert_eq!(
            what_the_refresh_came_to(&a_refresh(WHOEVER, "quarantined", "rotation-lost"), WHOEVER)
                .expect("a report that can be read"),
            Renewal::Lost
        );

        let came_to = what_the_refresh_came_to(
            &a_refresh(WHOEVER, "quarantined", "renewal-rejected"),
            WHOEVER,
        )
        .expect("a report that can be read");

        let Renewal::NotReached(why) = came_to else {
            panic!("a login ended elsewhere was read as Perch losing one: {came_to:?}");
        };
        assert!(why.contains("perch relogin"), "{why}");
    }

    /// Drift in Perch's own document, which is the one thing here that is a
    /// fault: a phase reading a key that has moved would otherwise go on to
    /// assert against a figure it never read.
    #[test]
    fn a_refresh_report_that_cannot_be_read_stops_the_run() {
        for document in [
            serde_json::json!({"refresh": null}),
            a_refresh("somebody@example.com", "observed", "null"),
            a_refresh(WHOEVER, "invented-since", "null"),
        ] {
            let Err(Halt::Stopped(setback)) = what_the_refresh_came_to(&document, WHOEVER) else {
                panic!("a report nothing could read was read anyway: {document}");
            };
            assert_eq!(setback.fault, Fault::Perch);
        }
    }

    // ---- a phase that gives up ---------------------------------------------

    /// The reading the whole of ADR 0038's ceiling rests on: a phase that
    /// discovers as it runs that this machine can prove nothing with it is a
    /// Skip, and a Skip is not a failure.
    #[test]
    fn a_phase_that_cannot_prove_anything_here_says_so_rather_than_passing() {
        let halted: Halt = Halt::not_here("nothing was stale");
        assert!(matches!(halted, Halt::Skipped(_)));

        // And the ordinary `?` on a command still means a stop, which is what
        // keeps every phase above readable.
        let stopped: Halt = Setback::perch("something is wrong").into();
        assert!(matches!(stopped, Halt::Stopped(_)));
    }
}
