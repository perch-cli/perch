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

use crate::commands::{Presumed, ask, ask_a_word, export, said_yes, say, still_ours};
use crate::error::{PerchError, Result};
use crate::host::{Host, Platform};
use crate::purge::{self, Purged};
use crate::registry::{self, Account, Registry};

/// The word, typed out. A letter is what fingers answer before eyes have read
/// anything, and this is the one command nothing undoes.
const THE_WORD: &str = "purge";

/// `yes` answers both questions ahead of time: purge, and write no Export.
pub fn run(host: &dyn Host, yes: bool, out: &mut dyn Write) -> Result<()> {
    // Before anything is read, because it is a refusal about this terminal
    // rather than about this machine.
    refuse_without_a_terminal_or_the_flag(host, yes)?;

    // Asked before the lock is taken, because taking it would create the very
    // directory this is looking for: Perch's home is made on the way to the lock
    // artifact that lives inside it.
    let home = registry::perch_home(host)?;
    if !host.path_exists(&home) {
        return Err(PerchError::NothingToDo(format!(
            "{} is not there, so Perch is holding nothing on this machine and \
             there is nothing to give back.",
            home.display(),
        )));
    }

    let mut perch = registry::lock(host)?;
    let (mut registry, readable) = whatever_can_be_read_of_the_registry(host, &home);

    purge::refuse_while_anything_is_running(host, &registry)?;

    say(
        out,
        &what_will_go(
            &registry,
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
    if !yes {
        // The offer's *own* failure carries the note as well, because the bytes
        // land before the report. Taken as a value first, so the borrow of
        // `exported` the call holds is over before the note reads it.
        let offered = offer_an_export(host, &mut perch, &mut registry, &home, &mut exported, out);
        offered.map_err(|error| still_standing(error, exported.as_deref()))?;
        // The note again, because the question *between* the offer and the
        // decision is a failure path of its own: `agreed` writes a prompt and
        // reads an answer, and a terminal that has gone away fails both.
        if !agreed(host, out).map_err(|error| still_standing(error, exported.as_deref()))? {
            // What the machine is holding now, which is not always nothing: an
            // Export written a question ago is a file full of working
            // Credentials at a path the user is about to stop thinking about.
            return match exported {
                Some(path) => say(
                    out,
                    &format!(
                        "Nothing was purged. The Export at {} was written before \
                         you declined and still stands — it holds a working \
                         Credential for every Account, so keep it somewhere you \
                         would keep those, or delete it. `perch holdings purge` \
                         will not write over it.",
                        path.display(),
                    ),
                ),
                None => say(out, "Nothing was purged."),
            };
        }
    }

    // Every failure from here on carries the same note the declined Purge does.
    // A Purge that finished says it in its own report instead.
    let and_the_export = |error: PerchError| still_standing(error, exported.as_deref());

    // The same guard `perch remove` takes, at the same point: the questions above
    // are the one wait in Perch with no bound on them, and a registry this Perch
    // may no longer write is one it may no longer act on either.
    still_ours(&mut perch, "purged").map_err(and_the_export)?;

    // Asked again over the same window as the hold: somebody may have started a
    // client while the passphrase was being typed. First *and* last, because the
    // first ask is what stops five questions to somebody this will refuse.
    purge::refuse_while_anything_is_running(host, &registry).map_err(and_the_export)?;

    // Before anything is deleted, and refused rather than continued if it will
    // not stop — ADR a-removal-lands-first at the scale of the whole machine. A
    // supervised Watcher comes straight back, and writes Credentials unasked.
    crate::commands::service::take_back_before_a_purge(host, out).map_err(and_the_export)?;

    let purged = purge::erase(host, &mut perch, &registry).map_err(and_the_export)?;
    report(host, out, &home, &purged, exported.as_deref())
}

/// Adds the whereabouts of an Export this run wrote to a failure after it.
///
/// Every way a Purge can stop past the offer leaves an armored file holding a
/// working Credential for every Account at a path nothing else mentions — and
/// `perch holdings export` refuses a path that is taken.
fn still_standing(error: PerchError, exported: Option<&std::path::Path>) -> PerchError {
    match exported {
        Some(path) => error.with_note(&format!(
            "The Export at {} was written before this stopped and still stands \
             — it holds a working Credential for every Account, so keep it \
             somewhere you would keep those, or delete it. `perch holdings \
             purge` will not write over it.",
            path.display(),
        )),
        None => error,
    }
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
                 under {} is emptied and deleted regardless — that is what a \
                 Purge does — and the count below is of Profiles rather than of \
                 Accounts.",
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
                 Nothing undoes it: only a fresh login brings an Account back, \
                 and it comes back as a new one.{and_the_service}",
                crate::commands::profiles(profiles),
                home.display(),
                // Only where the registry is the reason: one that parsed and
                // names nobody is the ordinary leftover of a login that died at
                // the browser step, and is not a corrupt file to be agreed to.
                match readable {
                    true => "",
                    false => " — its registry says nothing this Perch can read",
                },
                home.display(),
            ),
        };
    }

    format!(
        "Perch holds {}: {}.\n\
         A Purge deletes every one of their Profiles, every Credential Perch \
         holds for them, and {} itself. Nothing undoes it: only a fresh login \
         brings an Account back, and it comes back as a new one.\n\
         Claude Code goes on running as whatever it is logged in as — the live \
         Credential is not Perch's to take away.{and_the_service}",
        crate::commands::accounts(accounts.len()),
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
    home: &Path,
    landed: &mut Option<PathBuf>,
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
    if !said_yes(host, out, "Write an Export first? [Y/n]: ", Presumed::Yes)? {
        return Ok(());
    }

    let Some(path) = ask(host, out, "Where to write it: ")?
        .map(|typed| expanded(host, typed.trim()))
        .transpose()?
        .filter(|path| !path.as_os_str().is_empty())
    else {
        // Not read as a change of mind: somebody who has just asked for an Export
        // and named nowhere to put it is likelier to have hit the wrong key than
        // to have decided to give up every Credential without a copy.
        return Err(PerchError::Invalid(
            "No path was typed, so no Export was written.\n\
             Nothing was purged. Run `perch holdings purge` again — answering \
             `n` to the offer purges without one."
                .to_string(),
        ));
    };
    refuse_a_path_the_purge_would_take(host, &path, home)?;

    // The Export's own refusals are about the Export, and every one of them is
    // true. None says what the person typing `perch holdings purge` is waiting to
    // hear, which is whether the Purge happened.
    export::write_the_export(host, perch, registry, &path, landed, out).map_err(|error| {
        error.with_note(
            "Nothing was purged. Run `perch holdings purge` again — answering \
             `n` to the offer purges without an Export.",
        )
    })?;
    Ok(())
}

/// A path typed at a prompt, where a leading `~/` means what everybody who types
/// it means.
///
/// The one path in Perch no shell has been over. Only a leading `~/`, and every
/// other one is refused rather than read as a file beside the current directory.
fn expanded(host: &dyn Host, typed: &str) -> Result<PathBuf> {
    let Some(rest) = typed.strip_prefix('~') else {
        let typed = PathBuf::from(typed);
        // Resolved here rather than left for whoever writes it, because the
        // guard below matches components: a bare `backup.age` typed from inside
        // the directory this Purge is about to delete shares none with it.
        if typed.is_absolute() || typed.as_os_str().is_empty() {
            return Ok(typed);
        }
        // A machine that cannot say where it is says so through the guard
        // refusing nothing, which is where it stood before this line.
        return Ok(match host.current_dir() {
            Ok(here) => here.join(typed),
            Err(_) => typed,
        });
    };
    // Either separator, because Windows reads both and somebody typing here is
    // typing at Perch rather than at a shell — as `upgrade::beneath` and
    // `segments` already establish for every path Perch reads there.
    let separator = |character: char| {
        character == '/' || (host.platform() == Platform::Windows && character == '\\')
    };
    let Some(rest) = rest.strip_prefix(separator) else {
        return Err(PerchError::Invalid(format!(
            "`{typed}` begins with a `~` that does not name this machine's home, \
             and Perch will not read it as the name of a file — written where it \
             says, the Export would land in whatever directory you typed this \
             in.\n\
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

/// Refuses to write the Export inside the directory this Purge is about to delete.
///
/// Only the absolute case, which is the one somebody types here. Both sides
/// through every link, because `starts_with` matches components and a linked
/// spelling of one directory shares none of them.
fn refuse_a_path_the_purge_would_take(host: &dyn Host, path: &Path, home: &Path) -> Result<()> {
    if !crate::host::through_every_link(host, path)
        .starts_with(crate::host::through_every_link(host, home))
    {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "{} is inside {}, which this Purge is about to delete — so the Export \
         would go with it.\n\
         Nothing was purged. Name a path somewhere Perch does not own.",
        path.display(),
        home.display(),
    )))
}

/// Whether the Purge is to go ahead.
///
/// The word rather than a letter: `y` is what somebody answers before they have
/// read anything. Trimmed and case-folded, because the guard is against the
/// reflex rather than against the keyboard.
fn agreed(host: &dyn Host, out: &mut dyn Write) -> Result<bool> {
    let answered = ask_a_word(
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
    say(
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
                crate::commands::profiles(profiles),
                crate::commands::credentials(purged.unnamed.credentials),
                home.display(),
            ),
            (accounts, _) => format!(
                "Purged {}, and {} is gone.",
                crate::commands::accounts(accounts),
                home.display(),
            ),
        },
    )?;

    // Beside a count of Accounts, because that count does not include them.
    if purged.accounts > 0 && purged.unnamed.profiles > 0 {
        say(
            out,
            &format!(
                "{} under it named no Account, and {} deleted with them.",
                crate::commands::profiles(purged.unnamed.profiles),
                crate::commands::credentials(purged.unnamed.credentials),
            ),
        )?;
    }

    // The one thing here that is not what a Purge always does. What Claude Code
    // is still logged in as is said in the question this run was agreed to, which
    // is where it is load-bearing (ADR perch-says-what-it-did).
    if purged.credentials < purged.accounts {
        say(
            out,
            &format!(
                "{} of them had nothing in either Credential Store to delete — {}.",
                crate::commands::accounts(purged.accounts - purged.credentials),
                crate::commands::a_store_that_held_nothing(host),
            ),
        )?;
    }

    // The sentence every *other* way out of this command says about a file this
    // one wrote: the path is the only thing left that names the Holdings, and
    // the run that destroyed them is where it matters most.
    if let Some(path) = exported {
        say(
            out,
            &format!(
                "The Export is at {} — it holds a working Credential for every \
                 Account, so keep it somewhere you would keep those.",
                path.display(),
            ),
        )?;
    }

    Ok(())
}
