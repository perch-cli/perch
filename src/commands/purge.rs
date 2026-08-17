//! `perch holdings purge` — giving the machine back (ADR 0014).
//!
//! Every Profile, every Credential Perch holds, and Perch's own registry, gone
//! in one act. The exact inverse of an Import, and the command that makes moving
//! to another machine an act somebody can finish rather than start.
//!
//! **It takes no Target**, because it is never about one Account — that is
//! `perch remove`, which is deliberately narrow. Two verbs for one act is
//! exactly the ambiguity the shared Alias and Group namespace exists to prevent.
//!
//! **It offers an Export first.** Perch cannot verify that one exists — the file
//! is wherever the user put it, possibly on another machine — so requiring one
//! is a check that checks nothing. Offering to write one is not: it is a
//! keystroke, and it is the only artifact that makes a Purge survivable.
//! Declining is allowed and costs nothing but the offer.
//!
//! **The prompt is harder than every other prompt.** It lists what will go by
//! email address, says plainly that nothing undoes it, and wants the word
//! `purge` typed rather than a letter — because a `y` is what fingers answer
//! before eyes have read anything. `--yes` answers ahead of time for scripts,
//! the same idiom `perch remove` uses.
//!
//! It does not touch the Account that is currently active in the Default
//! Profile. Claude Code's own login is Claude Code's, and a Purge that logged
//! the user out of the tool they are using would be doing more than giving the
//! machine back.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::commands::{ask, ask_a_word, export, say, still_ours};
use crate::error::{PerchError, Result};
use crate::host::Host;
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
    // artifact that lives inside it, and a Purge that made a home in order to
    // report there was none would be leaving behind exactly what it promises to
    // take away.
    let home = registry::perch_home(host)?;
    if !host.path_exists(&home) {
        return Err(PerchError::NothingToDo(format!(
            "{} is not there, so Perch is holding nothing on this machine and \
             there is nothing to give back.",
            home.display(),
        )));
    }

    let mut perch = registry::lock(host)?;
    // Read directly rather than through adoption, for the reason an Import reads
    // it directly: adoption takes the existing Claude Code login over as the
    // first Profile, which here would be a Purge that made an Account on the way
    // to destroying every Account. A home holding no registry is what a Purge
    // interrupted during its last step leaves, and finishing that is the whole
    // of what running it again is for.
    let registry = registry::load(host)?.unwrap_or_default();

    purge::refuse_while_anything_is_running(host, &registry)?;

    say(
        out,
        &what_will_go(&registry, &home, crate::commands::service::is_there(host)),
    )?;
    // Filled by the Export the instant its bytes land, rather than by the call
    // returning: `write_the_export` reports to the terminal after the write, and
    // a terminal that has gone away fails that — which used to lose the note
    // saying the file is there, on the one path where that note is the whole of
    // what makes the Purge survivable.
    let mut exported = None;
    if !yes {
        // The offer's *own* failure carries the note too, which is the one arm
        // that could not before: `write_the_export` reports after the bytes
        // land, so this is exactly where a terminal that has gone away leaves a
        // whole Export behind and a refusal that does not mention it. Taken as
        // a value first, so the borrow of `exported` the call holds is over
        // before the note reads it.
        let offered = offer_an_export(host, &mut perch, &registry, &home, &mut exported, out);
        offered.map_err(|error| still_standing(error, exported.as_deref()))?;
        // The note again, because the question *between* the offer and the
        // decision is a failure path of its own: `agreed` writes a prompt and
        // reads an answer, and a terminal that has gone away — a closed pty, a
        // SIGHUP — fails both. Raised bare, this was the one way to stop
        // between the bytes landing and every note that mentions them, so
        // somebody read "could not read your answer" with no word of the
        // armored file holding a working Credential for every Account that had
        // just been written at a path nothing named — and the next `perch
        // holdings purge` aborted on that path, because an Export refuses an
        // occupied one.
        if !agreed(host, out).map_err(|error| still_standing(error, exported.as_deref()))? {
            // What the machine is holding now, which is not always nothing. An
            // Export written a question ago is a file full of working
            // Credentials sitting at a path the user is about to stop thinking
            // about — and `perch holdings export` refuses a path that is taken,
            // so the next Purge offering the same one aborts before it asks
            // anything. Both are things somebody has to be told to act on.
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

    // Every failure from here on carries the same note the declined Purge does,
    // and for the same two reasons: a file full of working Credentials is
    // sitting at a path the user is about to stop thinking about, and `perch
    // holdings export` refuses a path that is taken — so the next Purge
    // offering the same one aborts before it asks anything. Reported only where
    // the run stops before the Purge is complete: a Purge that finished says
    // where the file is in its own report.
    let and_the_export = |error: PerchError| still_standing(error, exported.as_deref());

    // The same guard `perch remove` takes, for the same reason and at the same
    // point: the questions above are the one wait in Perch with no bound on
    // them, and everything from this line on deletes Credentials. A registry
    // this Perch may no longer write is one it may no longer act on either.
    still_ours(&mut perch, "purged").map_err(and_the_export)?;

    // Asked again for the same reason the hold is re-checked, and it is the same
    // window: somebody may have started a client while the passphrase was being
    // typed, and the answer that was true before the questions says nothing
    // about the Profile this is about to delete. Asked first *and* last, because
    // the first ask is what stops a Purge putting five questions to somebody it
    // was always going to refuse.
    purge::refuse_while_anything_is_running(host, &registry).map_err(and_the_export)?;

    // Before anything is deleted, and refusing rather than continuing if it will
    // not stop (ADR 0040). A Watcher is the one process on this machine that
    // writes Credentials without being asked, and a supervised one comes
    // straight back — so "it will be gone in a moment" is not true of it. This
    // is ADR 0024's rule at the scale of the whole machine: land somewhere
    // before deleting anything.
    crate::commands::service::take_back_before_a_purge(host, out).map_err(and_the_export)?;

    let purged = purge::erase(host, &registry).map_err(and_the_export)?;
    report(host, out, &home, &purged)
}

/// Adds the whereabouts of an Export this run wrote to a failure that came
/// after it.
///
/// Only the decline arm used to say this, so every other way a Purge can stop —
/// a registry another Perch took over, a client started while the passphrase
/// was being typed, a Credential Store that would not empty — left an armored
/// file holding a working Credential for every Account at a path nothing had
/// mentioned. The next `perch holdings purge` then aborted on it before asking
/// anything, because `perch holdings export` refuses a path that is taken.
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
/// Every capability in Perch is reachable from a script (ADR 0011), so the
/// refusal names the flag rather than the terminal: `--yes` is the whole of what
/// a script needs, and a Purge that read end of input as agreement would be the
/// one command where a closed pipe costs every Credential on the machine.
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

/// What the user is being asked to agree to, in the terms they are deciding in:
/// which Accounts, by email address, and that nothing undoes it.
///
/// By email address rather than by Alias, although every other command names an
/// Account the way the user named it. An Alias is a convenience for typing, and
/// what is being agreed to here is the loss of the login itself — which is the
/// address, and is what somebody would have to check against their password
/// manager before typing the word.
fn what_will_go(registry: &Registry, home: &Path, service: bool) -> String {
    // Said in the same breath as the Profiles rather than left for the report,
    // because it is the one thing a Purge takes that lives *outside* Perch's
    // home (ADR 0040) — and consent to "everything under this directory" is not
    // consent to a file in `~/Library/LaunchAgents`.
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
        return format!(
            "Perch holds no Accounts here, so {} is all there is left to take.{and_the_service}",
            home.display(),
        );
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
/// Offered rather than required, because Perch cannot verify that an Export
/// exists: the file is wherever the user put it, possibly on another machine, so
/// requiring one is a check that checks nothing. The default is to write one —
/// declining is a keystroke, and so is agreeing, but only one of the two is
/// recoverable.
///
/// An Export that fails stops the Purge rather than being asked about again. A
/// path that is taken, a directory that is not there and a passphrase typed
/// twice differently are all things somebody has to go and settle, and nothing
/// has been destroyed yet — so the answer is to run `perch holdings purge`
/// again.
///
/// Returns where one was written, because the Purge it was offered for may
/// still be declined at the next question — and a file holding every Credential
/// on the machine is not something to leave a user unaware of on the strength
/// of "Nothing was purged."
fn offer_an_export(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    registry: &Registry,
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

    let answered = ask_a_word(host, out, "Write an Export first? [Y/n]: ")?;
    // A no is a no, and so is end of input: nobody who is not there to answer
    // this is there to type a passphrase at the two prompts that follow.
    // Everything else — including plain Return — writes one.
    if matches!(answered.as_deref(), None | Some("n" | "no")) {
        return Ok(());
    }

    let Some(path) = ask(host, out, "Where to write it: ")?
        .map(|typed| expanded(host, typed.trim()))
        .transpose()?
        .filter(|path| !path.as_os_str().is_empty())
    else {
        // Not read as a change of mind. Somebody who has just asked for an
        // Export and then named nowhere to put it is likelier to have hit the
        // wrong key than to have decided, between two prompts, to give up every
        // Credential on the machine without a copy — and the way back is one
        // keystroke shorter than the way forward would have been.
        return Err(PerchError::Invalid(
            "No path was typed, so no Export was written.\n\
             Nothing was purged. Run `perch holdings purge` again — answering \
             `n` to the offer purges without one."
                .to_string(),
        ));
    };
    refuse_a_path_the_purge_would_take(&path, home)?;

    // The Export's own refusals are about the Export — a path already taken, a
    // passphrase typed twice and differently, a registry another Perch has
    // since written. Every one of them is true and none of them says what the
    // person typing `perch holdings purge` is waiting to hear, which is whether
    // the Purge happened. Every other way this command stops says so; this was
    // the one that left them reading a sentence about a file and inferring the
    // rest.
    export::write_the_export(host, perch, registry, &path, landed, out).map_err(|error| {
        error.with_note(
            "Nothing was purged. Run `perch holdings purge` again — answering \
             `n` to the offer purges without an Export.",
        )
    })?;
    Ok(())
}

/// A path typed at a prompt, with a leading `~/` meaning what everybody who
/// types it means.
///
/// This is the one path in Perch that arrives without a shell having been over
/// it first. `perch holdings export ~/x.age` works because the shell expanded
/// the tilde before Perch ever saw it; the same characters read from standard
/// input are just a directory called `~`. Left alone, the likeliest answer to
/// "Where to write it:" was refused with "`~` is not a directory that exists",
/// the whole Purge stopped, and every question before it had to be answered
/// again — a refusal that reads as a bug rather than as an instruction.
///
/// Only a leading `~/`, and only against this machine's home. A `~` anywhere
/// else in a path is an ordinary character and is left alone.
///
/// Every other leading `~` is refused rather than taken verbatim. `~someone/` is
/// another user's home, which is a lookup Perch has no way to do; a bare `~` and
/// a `~backups` are a home directory somebody meant and a shell did not expand.
/// Passed through, all three named a file in whatever directory the command was
/// typed in — so the Export landed at `./~`, the report said "Exported 3
/// Accounts to ~", and the Purge that followed took every Credential on the
/// machine while the only copy of them sat somewhere nobody would look. This
/// prompt is the one path in Perch a shell has not been over, which is exactly
/// why it cannot read an unexpanded `~` as a filename.
fn expanded(host: &dyn Host, typed: &str) -> Result<PathBuf> {
    let Some(rest) = typed.strip_prefix('~') else {
        return Ok(PathBuf::from(typed));
    };
    let Some(rest) = rest.strip_prefix('/') else {
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
        // command at `perch_home`, so this is unreachable rather than a
        // fallback. Verbatim is the honest answer for it either way: the
        // refusal that follows names the `~` the user typed.
        Err(_) => Ok(PathBuf::from(typed)),
    }
}

/// Refuses to write the Export inside the directory this Purge is about to
/// delete.
///
/// The file is the only reason the Purge is survivable, and one written under
/// Perch's home would be taken by the Purge that offered it moments later. Only
/// the absolute case is caught, which is the one somebody types at this prompt;
/// a relative path that resolves under the home is a stranger thing to type than
/// this check is worth making complicated for.
fn refuse_a_path_the_purge_would_take(path: &Path, home: &Path) -> Result<()> {
    if !path.starts_with(home) {
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
/// The word rather than a letter, and the difference is the point: `y` is what
/// somebody answers before they have read anything, and this is the one command
/// nothing undoes. Trimmed and case-folded, because a purge refused over a
/// trailing space or a Caps Lock would only be typed again — the guard is
/// against the reflex, not against the keyboard.
fn agreed(host: &dyn Host, out: &mut dyn Write) -> Result<bool> {
    let answered = ask_a_word(
        host,
        out,
        &format!("Type `{THE_WORD}` to give the machine back: "),
    )?;
    Ok(answered.as_deref() == Some(THE_WORD))
}

/// What was given back, and what is still the machine's.
fn report(host: &dyn Host, out: &mut dyn Write, home: &Path, purged: &Purged) -> Result<()> {
    // Said as what happened rather than as a count, because "Purged 0 Accounts"
    // is not a sentence — and holding none is a real state here: it is what a
    // Purge that stopped in its last step leaves for the next one to finish.
    say(
        out,
        &match purged.accounts {
            0 => format!(
                "Perch was holding no Accounts here, so {} was all there was left \
                 to take, and it is gone.",
                home.display(),
            ),
            accounts => format!(
                "Purged {}. Every Profile, every Credential Perch held and {} \
                 are gone, and Perch is holding nothing on this machine.",
                crate::commands::accounts(accounts),
                home.display(),
            ),
        },
    )?;

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

    say(
        out,
        "Claude Code is still logged in as whatever it was: the live Credential \
         was not Perch's to take away. `perch holdings import` puts an Export \
         back on a machine like this one.",
    )
}
