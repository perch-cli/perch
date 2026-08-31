//! `perch holdings purge` — giving the machine back
//! (ADR the-holdings-go-out-sealed).
//!
//! Every Profile, every Credential Perch holds, and Perch's own registry, gone
//! in one act. The exact inverse of an Import, and the command that makes moving
//! to another machine an act somebody can finish rather than start.
//!
//! It takes no Target, it offers an Export first, its prompt wants the word
//! `purge` typed out rather than a letter, and it leaves the Account that is
//! active in the Default Profile exactly where it is.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::ask;
use crate::commands::{export, still_ours};
use crate::credentials;
use crate::error::{PerchError, Result};
use crate::holdings;
use crate::host::{Host, Platform};
use crate::probe::Installed;
use crate::purge::{self, Purged};
use crate::registry::{self, Account, Registry};
use crate::say;
use crate::wait;

/// The word, typed out. A letter is what fingers answer before eyes have read
/// anything, and this is the one command nothing undoes.
const THE_WORD: &str = "purge";

/// What to do about a Purge that stopped over the Export it offered. Said in
/// one place because both ways out of the offer end here, and the two are one
/// instruction.
const RUN_IT_AGAIN: &str = "Nothing was purged. Run `perch holdings purge` again; \
     answering `n` to the offer purges without one.";

/// What agreeing to a Purge costs, said in one place: both of the sentences the
/// question can open with end on it, and two copies would sooner or later be
/// two different promises about the same act.
const NOTHING_UNDOES_IT: &str = "Nothing undoes it: only a fresh login brings an \
     Account back, and it comes back as a new one.";

/// `yes` answers both questions ahead of time: purge, and write no Export.
pub fn run(host: &dyn Host, yes: bool, out: &mut dyn Write) -> Result<()> {
    // Before anything is read, because it is a refusal about this terminal
    // rather than about this machine.
    refuse_without_a_terminal_or_the_flag(host, yes)?;

    // Asked before the lock is taken, because taking it would create the very
    // directory this is looking for: Perch's home is made on the way to the lock
    // artifact that lives inside it.
    let home = holdings::perch_home(host)?;
    if !host.path_exists(&home) {
        return Err(PerchError::NothingToDo(format!(
            "{} is not there, so Perch is holding nothing on this machine and \
             there is nothing to give back.",
            home.display(),
        )));
    }

    let perch = holdings::lock(host)?;
    let (registry, readable) = whatever_can_be_read_of_the_registry(host, &home);

    let installed = Installed::for_a_report(host);

    let mut holding = (perch, registry);
    // The hold first, the other way round from `perch remove`: a registry this
    // Perch may no longer write is one it may no longer act on either.
    let mut standing = wait::Standing::of()
        .and(|(perch, _): &mut (crate::lock::Held<'_>, Registry)| still_ours(perch, "purged"))
        .and(|(_, registry)| purge::refuse_while_anything_is_running(host, registry, &installed));
    // Before the offer as well as after the question, because the ask up front
    // is what stops five questions to somebody this will refuse.
    standing.establish(&mut holding)?;

    say::line(
        out,
        &what_will_go(
            &holding.1,
            &home,
            purge::profiles_held(host).unwrap_or(0),
            crate::commands::service::is_there(host),
            readable,
        ),
    )?;
    // Filled by the Export the instant its bytes land rather than by the call
    // returning: `write_the_export` reports after the write, and a terminal that
    // has gone away fails that.
    let mut exported = None;
    let crossed = wait::across_unless_declined(
        &mut holding,
        |(perch, registry)| {
            if yes {
                return Ok(wait::Asked::Answered(()));
            }
            offer_an_export(host, perch, registry, &mut exported, &installed, out)?;
            Ok(match agreed(host, out)? {
                true => wait::Asked::Answered(()),
                false => wait::Asked::Declined,
            })
        },
        |holding| standing.establish(holding),
    );
    let (mut perch, registry) = holding;
    // Every failure out of the wait and on from it carries the whereabouts of
    // the Export, because the bytes land before the offer's report. A Purge
    // that finished says it in its own report instead.
    let and_the_export = |error: PerchError| still_standing(error, exported.as_deref());
    let Some(((), (), fresh)) = crossed.map_err(and_the_export)? else {
        // What the machine is holding now, which is not always nothing: an
        // Export written a question ago is a file full of working
        // Credentials at a path the user is about to stop thinking about.
        return match exported {
            Some(path) => say::line(
                out,
                &format!("Nothing was purged. {}", the_export_is_at(&path)),
            ),
            None => say::line(out, "Nothing was purged."),
        };
    };

    // Before anything is deleted, and refused rather than continued if it will
    // not stop — ADR a-removal-lands-first at the scale of the whole machine. A
    // supervised Watcher comes straight back, and writes Credentials unasked.
    crate::commands::service::take_back_before_a_purge(host, out, &fresh)
        .map_err(and_the_export)?;

    let purged = purge::erase(host, &mut perch, &registry, &fresh).map_err(and_the_export)?;
    // The Export's whereabouts is the report's *last* line, so a report that
    // failed before it leaves the Holdings gone and the file holding them named
    // nowhere. What the note adds is that there is nothing to run again.
    report(host, out, &home, &purged, exported.as_deref()).map_err(|error| {
        and_the_export(error.with_note(
            "The Purge itself finished: the Holdings are gone, and only the \
             report could not be printed.",
        ))
    })
}

/// Adds the whereabouts of an Export this run wrote to a failure after it.
///
/// Every way a Purge can stop past the offer leaves an armored file holding a
/// working Credential for every Account at a path nothing else mentions — and
/// `perch holdings export` refuses a path that is taken.
fn still_standing(error: PerchError, exported: Option<&std::path::Path>) -> PerchError {
    match exported {
        Some(path) => error.with_note(&the_export_is_at(path)),
        None => error,
    }
}

/// Where an Export this run wrote is, and what it holds.
///
/// One sentence in one place: a Purge names the file when it is declined, when
/// it fails and when it finishes, and those three differ only in what they say
/// happened around it.
fn the_export_is_at(path: &Path) -> String {
    format!(
        "The Export is at {}, and holds a working Credential for every Account. \
         Keep it somewhere you would keep those. `perch holdings purge` will not \
         write over it.",
        path.display(),
    )
}

/// Refuses a Purge nobody is there to agree to.
///
/// Every capability is reachable from a script (ADR perch-does-not-draw), so the
/// refusal names the flag rather than the terminal: a Purge reading end of input
/// as agreement is where a closed pipe costs every Credential on the machine.
fn refuse_without_a_terminal_or_the_flag(host: &dyn Host, yes: bool) -> Result<()> {
    if yes || host.is_interactive() {
        return Ok(());
    }
    Err(PerchError::Invalid(
        "There is no terminal to confirm on, and a Purge deletes every Profile, \
         every Credential Perch holds and its own registry.\n\
         Nothing was purged. Pass `--yes` to purge without being asked."
            .to_string(),
    ))
}

/// What the registry names, where it can be read at all.
///
/// The one caller for which `load`'s refusal is the wrong answer: `erase` walks
/// the directories rather than the registry, and refusing is the only way off a
/// machine whose registry is corrupt (ADR the-holdings-outlive-a-perch).
fn whatever_can_be_read_of_the_registry(host: &dyn Host, home: &Path) -> (Registry, bool) {
    // Read directly rather than through adoption, for the reason an Import reads
    // it directly: adoption would make an Account on the way to destroying every
    // Account. A home holding no registry is what an interrupted Purge leaves.
    match registry::load(host) {
        Ok(held) => (held.unwrap_or_default(), true),
        Err(unreadable) => {
            host.note(&format!(
                "{unreadable}\n\nSo the Accounts cannot be named. Every Profile \
                 under {} is emptied and deleted regardless, and the count below \
                 is of Profiles rather than of Accounts.",
                home.display(),
            ));
            (Registry::default(), false)
        }
    }
}

/// What the user is being asked to agree to, in the terms they are deciding in:
/// which Accounts, by email address, and that nothing undoes it.
///
/// By address rather than by Alias, although every other command names an Account
/// the way the user named it: what is being agreed to is the loss of the login.
fn what_will_go(
    registry: &Registry,
    home: &Path,
    profiles: usize,
    service: bool,
    readable: bool,
) -> String {
    // Said in the same breath as the Profiles rather than left for the report,
    // because it is the one thing a Purge takes that lives *outside* Perch's home
    // (ADR the-machine-runs-the-watcher).
    let and_the_service = match service {
        true => {
            "\nThe Service goes too, and goes first: nothing may be \
                 Switching Credentials into Profiles this is deleting. Its unit \
                 is removed, so nothing starts at your next login."
        }
        false => "",
    };

    let accounts: Vec<&str> = registry.accounts.iter().map(Account::email).collect();
    if accounts.is_empty() {
        // The Profiles rather than the Accounts, because a registry naming none of
        // them is the state where that count is the only one there is — and an
        // unparsable one must not be agreed to as an empty machine.
        return match profiles {
            0 => format!(
                "Perch holds no Accounts here, so {} is all there is left to \
                 take.{and_the_service}",
                home.display(),
            ),
            profiles => format!(
                "Perch holds {} under {} that it cannot name{}. A Purge empties \
                 every one of their Credential Stores and deletes {} itself. \
                 {NOTHING_UNDOES_IT}{and_the_service}",
                say::profiles(profiles),
                home.display(),
                // Only where the registry is the reason: one that parsed and
                // names nobody is the ordinary leftover of a login that died at
                // the browser step, and is not a corrupt file to be agreed to.
                match readable {
                    true => "",
                    false => ", its registry saying nothing this Perch can read",
                },
                home.display(),
            ),
        };
    }

    format!(
        "Perch holds {}: {}.\n\
         A Purge deletes every one of their Profiles, every Credential Perch \
         holds for them, and {} itself. {NOTHING_UNDOES_IT}\n\
         Claude Code goes on running as whatever it is logged in as.\
         {and_the_service}",
        say::accounts(accounts.len()),
        accounts.join(", "),
        home.display(),
    )
}

/// Offers the one thing that makes a Purge survivable, and takes no for an
/// answer.
///
/// An Export that fails stops the Purge rather than being asked about again, and
/// returns where one was written, because the Purge may still be declined.
fn offer_an_export(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &mut Registry,
    landed: &mut Option<PathBuf>,
    installed: &Installed,
    out: &mut dyn Write,
) -> Result<()> {
    // Nothing to put in one. `perch holdings export` refuses this too, and
    // meeting that refusal here would be a Purge failing over an offer it
    // should not have made.
    if registry.accounts.is_empty() {
        return Ok(());
    }

    // Nobody who is not there to answer this is there to type a passphrase at
    // the two prompts that follow, so end of input declines rather than taking
    // the `[Y/n]`.
    if !ask::said_yes(
        host,
        out,
        "Write an Export first? [Y/n]: ",
        ask::Presumed::Yes,
    )? {
        return Ok(());
    }

    let Some(path) = ask::line(host, out, "Where to write it: ")?
        .map(|typed| expanded(host, typed.trim()))
        .transpose()?
        .filter(|path| !path.as_os_str().is_empty())
    else {
        // Not read as a change of mind: somebody who has just asked for an Export
        // and named nowhere to put it is likelier to have hit the wrong key than
        // to have decided to give up every Credential without a copy.
        return Err(PerchError::Invalid(format!(
            "No path was typed, so no Export was written.\n{RUN_IT_AGAIN}"
        )));
    };
    // The Export's own refusals are about the Export, and every one of them is
    // true. None says what the person typing `perch holdings purge` is waiting to
    // hear, which is whether the Purge happened.
    let noted = |error: PerchError| error.with_note(RUN_IT_AGAIN);
    let mut destination = export::Destination::for_an_export(host, &path).map_err(noted)?;
    let written = export::write_the_export(host, perch, registry, &mut destination, installed, out);
    // Read off the Destination whether the call refused or not: the bytes land
    // before the report, and a terminal that has gone away fails the report.
    *landed = destination.landed().map(Path::to_path_buf);
    written.map_err(noted)
}

/// A path typed at a prompt, where a leading `~/` means what everybody who types
/// it means.
///
/// The one path in Perch no shell has been over. Only a leading `~/`, and every
/// other one is refused rather than read as a file beside the current directory.
fn expanded(host: &dyn Host, typed: &str) -> Result<PathBuf> {
    let on_windows = host.platform() == Platform::Windows;
    let Some(rest) = typed.strip_prefix('~') else {
        // Rooted asked of the platform the *Host* reports, and joined with `/`
        // by hand, for `probe::rooted`'s reason: `is_absolute` and `join` read
        // the separator of the platform this build runs on.
        if typed.is_empty() || crate::probe::rooted(typed, on_windows) {
            return Ok(PathBuf::from(typed));
        }
        // Resolved rather than left for whoever writes it, because the guard
        // below matches components: a bare `backup.age` typed inside the
        // directory this Purge deletes shares none with it.
        return Ok(match host.current_dir() {
            Ok(here) => PathBuf::from(format!("{}/{typed}", here.display())),
            Err(_) => PathBuf::from(typed),
        });
    };
    // Either separator, because Windows reads both and somebody typing here is
    // typing at Perch rather than at a shell.
    let separator = |character: char| character == '/' || (on_windows && character == '\\');
    let Some(rest) = rest.strip_prefix(separator) else {
        return Err(PerchError::Invalid(format!(
            "`{typed}` begins with a `~` that does not name this machine's home, \
             and Perch will not read it as the name of a file.\n\
             Nothing was purged. Run `perch holdings purge` again and name a \
             path, such as `~/perch.age`."
        )));
    };
    match host.home_dir() {
        Ok(home) => Ok(home.join(rest)),
        // A machine that cannot say where home is has already refused this
        // command at `perch_home`, so this is unreachable rather than a fallback.
        // Verbatim is honest either way: the refusal names the `~` that was typed.
        Err(_) => Ok(PathBuf::from(typed)),
    }
}

/// Whether the Purge is to go ahead.
///
/// The word rather than a letter: `y` is what somebody answers before they have
/// read anything. Trimmed and case-folded, because the guard is against the
/// reflex rather than against the keyboard.
fn agreed(host: &dyn Host, out: &mut dyn Write) -> Result<bool> {
    let answered = ask::a_word(
        host,
        out,
        &format!("Type `{THE_WORD}` to give the machine back: "),
    )?;
    Ok(answered.as_deref() == Some(THE_WORD))
}

/// What was given back.
fn report(
    host: &dyn Host,
    out: &mut dyn Write,
    home: &Path,
    purged: &Purged,
    exported: Option<&Path>,
) -> Result<()> {
    // Said as what happened rather than as a count, because "Purged 0 Accounts"
    // is not a sentence — and holding none is a real state here: it is what a
    // Purge that stopped in its last step leaves for the next one to finish.
    say::line(
        out,
        &match (purged.accounts, purged.unnamed.profiles) {
            (0, 0) => format!(
                "Perch was holding no Accounts here, so {} was all there was left \
                 to take, and it is gone.",
                home.display(),
            ),
            // The Profiles, because the registry named no Account and they are
            // the only count there is. Never "no Accounts": a machine whose
            // registry would not parse still held every one of these.
            (0, profiles) => format!(
                "Purged {} Perch could not name, {} among them, and {} is gone.",
                say::profiles(profiles),
                say::credentials(purged.unnamed.credentials),
                home.display(),
            ),
            (accounts, _) => format!(
                "Purged {}, and {} is gone.",
                say::accounts(accounts),
                home.display(),
            ),
        },
    )?;

    // Beside a count of Accounts, because that count does not include them.
    if purged.accounts > 0 && purged.unnamed.profiles > 0 {
        say::line(
            out,
            &format!(
                "{} under it named no Account, and {} deleted with them.",
                say::profiles(purged.unnamed.profiles),
                say::credentials(purged.unnamed.credentials),
            ),
        )?;
    }

    // The one thing here that is not what a Purge always does. What Claude Code
    // is still logged in as is said in the question this run was agreed to, which
    // is where it is load-bearing (ADR perch-says-what-it-did).
    if purged.credentials < purged.accounts {
        say::line(
            out,
            &format!(
                "{} of them had nothing in either Credential Store to delete, and {}.",
                say::accounts(purged.accounts - purged.credentials),
                credentials::a_store_that_held_nothing(host),
            ),
        )?;
    }

    // The sentence every *other* way out of this command says about a file this
    // one wrote: the path is the only thing left that names the Holdings, and
    // the run that destroyed them is where it matters most.
    if let Some(path) = exported {
        say::line(out, &the_export_is_at(path))?;
    }

    Ok(())
}
