//! `perch switch [<target>]` — make an Account active everywhere.
//!
//! Every client reads the same Default Profile, so one Switch moves the terminal
//! you are in, the ones you are not, the editor extension and the desktop app
//! together. The work itself and every refusal that protects it is
//! [`crate::switch`]'s; what lives here is deciding which Account was meant,
//! declining to do it again, and saying where you landed. With no Target the
//! Account is chosen rather than named — a Cycle within the current Account's
//! Group ([`crate::cycle`]) — and it asks nothing, ever
//! (ADR perch-does-not-draw).

use std::io::Write;

use crate::adopt;
use crate::cycle;
use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::live;
use crate::probe::Installed;
use crate::registry::{self, Account, Registry, Scope, Settled};
use crate::say;
use crate::switch::{self, Captured, Switched};
use crate::target::{self, Target};
use crate::utilization;

#[derive(Debug, Clone, clap::Args)]
pub struct SwitchArgs {
    /// The Account to switch to — its Alias or its email address — or a
    /// Group to Cycle within.
    pub target: Option<String>,

    /// Cycle on the cached figures, without reading any first.
    ///
    /// A Cycle otherwise reads the Accounts it cannot rank without, which
    /// costs a round trip each. Nothing is read for a named Account
    /// either way, because naming one decides nothing.
    #[arg(long)]
    pub no_refresh: bool,
}

/// The Account to switch to, and what deciding on it left to be said.
struct Decision {
    incoming: Account,
    /// What a Cycle chose this Account on, and the Scope it stayed inside, ready
    /// to go on the end of the landing line. Absent when somebody named the
    /// Account, because then nothing was chosen (ADR perch-says-what-it-did).
    chosen: Option<String>,
    /// The Accounts this Cycle wanted to read and could not, said in the words
    /// the failure used. Empty is every figure it ranked on either current or
    /// proven unable to change the answer, which is the ordinary case and says
    /// nothing.
    unread: Vec<String>,
}

pub fn run(host: &dyn Host, args: SwitchArgs, out: &mut dyn Write) -> Result<()> {
    let (mut perch, mut registry) = adopt::ensure_adopted_exclusively(host)?;

    let settled = crate::commands::a_settled_landing(host, &mut perch, &mut registry)?;

    // Read once, for the whole command: the refusals below and the Switch after
    // them name the Claude Code they were reading in anything they refuse
    // (ADR an-assumption-is-probed). Ahead of the Cycle now, which refuses on it.
    let installed = Installed::probed(host)?;

    let Decision {
        incoming,
        chosen,
        unread,
    } = decide(
        host,
        &mut perch,
        &mut registry,
        &settled,
        &args,
        &installed,
        out,
    )?;
    let outgoing = registry.active_account(&settled).cloned();

    already_there(host, &installed, &registry, &settled, &incoming)?;

    // Everything the Switch owes the registry is written by `switch_to`, which
    // is the only way to reach what the Switch found. `Reason::Asked` is all
    // this caller differs by: somebody typed it, so nothing paces anything.
    let Switched { captured, .. } = switch::switch_to(
        host,
        &mut perch,
        &mut registry,
        &installed,
        &incoming,
        outgoing.as_ref(),
        switch::Reason::Asked,
    )?;

    report(
        out,
        &registry,
        &incoming,
        chosen.as_deref(),
        &unread,
        &captured,
        host.now(),
    )
}

/// Which Account this Switch is for, and how it was arrived at.
///
/// A Target naming a Group names a set of Accounts declared interchangeable,
/// which is what a Cycle needs — so the three forms differ only in how the
/// Account is arrived at, and the Switch that follows is the same one.
fn decide(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    settled: &Settled,
    args: &SwitchArgs,
    installed: &Installed,
    out: &mut dyn Write,
) -> Result<Decision> {
    let scope = match args.target.as_deref() {
        Some(target) => {
            let found = target::resolve(registry, target)?;
            say::line(out, &found.matched())?;
            match found {
                Target::Group { name } => Scope::Group(name),
                Target::Alias { email, .. } | Target::Account { email } => {
                    let incoming = registry.held(&email)?.clone();
                    refuse_a_quarantined_account(registry, &incoming)?;
                    return Ok(Decision {
                        incoming,
                        chosen: None,
                        unread: Vec::new(),
                    });
                }
            }
        }
        // Never outside the Group the current Account is in, so a work
        // subscription running dry does not land on a personal Account.
        None => cycle::scope_for(registry, leaving(registry, settled)?)?,
    };

    let (figures, unread) =
        read_what_it_cannot_rank(host, perch, registry, settled, args, installed, &scope)?;

    // Nothing is set aside: a Cycle somebody asked for is one they get, and the
    // margin and the cooldown are the Watcher's rules for acting unasked
    // (ADR a-watcher-knob-is-arithmetic).
    let choice = cycle::choose(
        registry,
        &scope,
        registry.active().whose(),
        &cycle::SetAside::nothing(),
        figures,
        host.now(),
    )?;
    Ok(Decision {
        chosen: Some(choice.basis.in_the(&scope)),
        incoming: choice.account,
        unread,
    })
}

/// Reads the Accounts this Cycle cannot rank without, and no others
/// (ADR a-choice-reads-what-it-ranks). Two passes, because the second's question
/// is the first's answer: the Account being left is what every rival is measured
/// against, so it is read before there is anything to measure them against.
fn read_what_it_cannot_rank(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    settled: &Settled,
    args: &SwitchArgs,
    installed: &Installed,
    scope: &Scope,
) -> Result<(cycle::Figures, Vec<String>)> {
    if args.no_refresh {
        return Ok((cycle::Figures::Cached, Vec::new()));
    }

    let leaving = registry.active_account(settled).cloned();
    // Before a single allowance is spent, because a Switch off a Profile a
    // client is running against is refused whatever the figures say
    // (ADR a-profile-is-live-by-evidence).
    if let Some(leaving) = &leaving {
        live::ask(host, &[live::Place::of_the_profile(host, leaving)?])
            .idle_or(installed, &live::NOTHING_WAS_CHANGED)?;
    }

    let mut unread = Vec::new();
    if let Some(leaving) = &leaving
        && cycle::trusted(leaving, host.now()).is_none()
    {
        let about = [leaving.email().to_string()];
        unread.extend(crate::commands::read_now(host, perch, registry, &about).notes());
    }

    let leaving = leaving.as_ref().map(Account::email);
    let to_beat = leaving
        .and_then(|leaving| registry.account(leaving))
        .and_then(|account| cycle::trusted(account, host.now()));
    let rivals = cycle::worth_reading(registry, scope, leaving, to_beat, host.now());
    unread.extend(crate::commands::read_now(host, perch, registry, &rivals).notes());

    Ok((cycle::Figures::Current, unread))
}

/// Refuses to make a Credential live that is known not to work.
///
/// Cycling has never been able to choose a Quarantined Account; naming one is
/// where the user would otherwise find out by losing the session they were in. A
/// code of its own, because no other refusal is answered by logging in again.
pub(crate) fn refuse_a_quarantined_account(registry: &Registry, incoming: &Account) -> Result<()> {
    registry::refuse_a_quarantined_account(
        registry,
        incoming.email(),
        "Nothing was changed — switching to it would make a Credential live \
         that no longer works, and cost you the Account you are on.",
    )
}

/// The Account a bare `perch switch` would be leaving, which is the one whose
/// Group decides where it may look.
fn leaving<'a>(registry: &'a Registry, settled: &Settled) -> Result<&'a Account> {
    registry.active_account(settled).ok_or_else(|| {
        registry::no_active_account(registry, ", so there is no Group to Cycle within")
    })
}

/// The refusal to rewrite Credentials for nothing, when there is nothing to do.
///
/// Perch's own record is not enough to establish that: a Switch interrupted
/// between writing the Credential and patching the Identity is recorded as
/// active while Claude Code still names somebody else.
fn already_there(
    host: &dyn Host,
    installed: &Installed,
    registry: &Registry,
    settled: &Settled,
    incoming: &Account,
) -> Result<()> {
    if !registry.is_active(settled, incoming.email()) {
        return Ok(());
    }
    if !switch::already_landed(host, installed, incoming)? {
        return Ok(());
    }

    Err(PerchError::NothingToDo(format!(
        "{} is already the active Account. Nothing was changed.",
        registry.named_for_the_user(incoming.email())
    )))
}

fn report(
    out: &mut dyn Write,
    registry: &Registry,
    incoming: &Account,
    chosen: Option<&str>,
    unread: &[String],
    captured: &Captured,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    match captured {
        // Said by nothing, because it happens before every Switch without
        // exception — the ordinary case announcing that it was ordinary. The
        // reassurance is the guide's to give once.
        Captured::Copied { .. } => {}
        // The one case where a Capture was declined rather than found
        // unnecessary, so it says what was live and what was spared.
        Captured::NotTheirs { outgoing, live } => say::line(
            out,
            &format!(
                "The live Credential names {live}, not {outgoing}, so it was not \
                 Captured — {outgoing}'s own Credential is untouched. A login \
                 made outside Perch is not kept: run `perch add` before \
                 switching to keep one."
            ),
        )?,
        // The one case where switching back to that Account needs a login
        // rather than just working.
        Captured::NothingLive => say::line(
            out,
            "There was no live Credential to Capture — Claude Code was logged out.",
        )?,
        // The live store held something that was not a Credential. Said rather
        // than swallowed, and not refused either: bytes nothing can read are not
        // a Rotation, and this Switch puts a usable Credential back in front.
        Captured::Unreadable { outgoing, why } => say::line(
            out,
            &format!(
                "The live Credential could not be read, so it was not Captured \
                 and {outgoing}'s own Credential is untouched: {why}"
            ),
        )?,
        // Also worth saying: whatever was live belonged to no Account Perch
        // holds, so it was replaced rather than kept anywhere.
        Captured::NoOutgoing => say::line(
            out,
            "Perch held no active Account, so there was nothing to Capture.",
        )?,
        // A `perch run` against the Account being left Rotated its own Profile's
        // copy, so the live one is the older. Said, because it is the one case
        // where the Account keeps a Credential the live store never held.
        Captured::Superseded { outgoing } => say::line(
            out,
            &format!(
                "{outgoing}'s Profile already held a newer Credential than the \
                 live one, so it was kept rather than Captured over."
            ),
        )?,
        // The repair for a Switch that stopped before it named the Account it
        // had landed on. Nothing was Captured because nothing had moved on.
        Captured::NothingToSave => say::line(
            out,
            &format!(
                "{}'s Credential was already the live one, so there was nothing \
                 to Capture — this finished a Switch that had stopped before \
                 naming it.",
                incoming.email(),
            ),
        )?,
    }

    // An Account the Cycle wanted to read and could not, ranked on whatever was
    // cached — the one thing that can make this Switch land somewhere worse than
    // it left. Silence is every figure it ranked on current or proven harmless.
    for note in unread {
        say::line(out, note)?;
    }

    // Where it landed, and — where the Account was chosen rather than named —
    // what it was chosen on and the Scope the Cycle stayed inside. One line,
    // because the ranking is not worth defending.
    let named = registry.named_for_the_user(incoming.email());
    say::line(
        out,
        &match chosen {
            Some(chosen) => format!("Switched to {named}, {chosen}."),
            None => format!("Switched to {named}."),
        },
    )?;

    // As of the cache and never from the network
    // (ADR a-figure-carries-its-age): the figures are shown with their age, so a
    // stale one reads as stale rather than as a promise.
    utilization::write_figures(out, incoming, now)
}
