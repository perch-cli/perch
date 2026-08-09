//! `perch purge` — giving the machine back (ADR 0014).
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

#[derive(Debug, Clone)]
pub struct PurgeArgs {
    /// Answer both questions ahead of time: purge, and write no Export.
    pub yes: bool,
}

/// The word, typed out. A letter is what fingers answer before eyes have read
/// anything, and this is the one command nothing undoes.
const THE_WORD: &str = "purge";

pub fn run(host: &dyn Host, args: PurgeArgs, out: &mut dyn Write) -> Result<()> {
    // Before anything is read, because it is a refusal about this terminal
    // rather than about this machine.
    refuse_without_a_terminal_or_the_flag(host, args.yes)?;

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

    say(out, &what_will_go(&registry, &home))?;
    if !args.yes {
        let exported = offer_an_export(host, &mut perch, &registry, &home, out)?;
        if !agreed(host, out)? {
            // What the machine is holding now, which is not always nothing. An
            // Export written a question ago is a file full of working
            // Credentials sitting at a path the user is about to stop thinking
            // about — and `perch export` refuses a path that is taken, so the
            // next Purge offering the same one aborts before it asks anything.
            // Both are things somebody has to be told to act on.
            return match exported {
                Some(path) => say(
                    out,
                    &format!(
                        "Nothing was purged. The Export at {} was written before \
                         you declined and still stands — it holds a working \
                         Credential for every Account, so keep it somewhere you \
                         would keep those, or delete it. `perch purge` will not \
                         write over it.",
                        path.display(),
                    ),
                ),
                None => say(out, "Nothing was purged."),
            };
        }
    }

    // The same guard `perch remove` takes, for the same reason and at the same
    // point: the questions above are the one wait in Perch with no bound on
    // them, and everything from this line on deletes Credentials. A registry
    // this Perch may no longer write is one it may no longer act on either.
    still_ours(&mut perch, "purged")?;

    // Asked again for the same reason the hold is re-checked, and it is the same
    // window: somebody may have started a client while the passphrase was being
    // typed, and the answer that was true before the questions says nothing
    // about the Profile this is about to delete. Asked first *and* last, because
    // the first ask is what stops a Purge putting five questions to somebody it
    // was always going to refuse.
    purge::refuse_while_anything_is_running(host, &registry)?;

    let purged = purge::erase(host, &registry)?;
    report(out, &home, &purged)
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
fn what_will_go(registry: &Registry, home: &Path) -> String {
    let accounts: Vec<&str> = registry.accounts.iter().map(Account::email).collect();
    if accounts.is_empty() {
        return format!(
            "Perch holds no Accounts here, so {} is all there is left to take.",
            home.display(),
        );
    }

    format!(
        "Perch holds {}: {}.\n\
         A Purge deletes every one of their Profiles, every Credential Perch \
         holds for them, and {} itself. Nothing undoes it: only a fresh login \
         brings an Account back, and it comes back as a new one.\n\
         Claude Code goes on running as whatever it is logged in as — the live \
         Credential is not Perch's to take away.",
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
/// has been destroyed yet — so the answer is to run `perch purge` again.
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
    out: &mut dyn Write,
) -> Result<Option<PathBuf>> {
    // Nothing to put in one. `perch export` refuses this too, and meeting that
    // refusal here would be a Purge failing over an offer it should not have
    // made.
    if registry.accounts.is_empty() {
        return Ok(None);
    }

    let answered = ask_a_word(host, out, "Write an Export first? [Y/n]: ")?;
    // A no is a no, and so is end of input: nobody who is not there to answer
    // this is there to type a passphrase at the two prompts that follow.
    // Everything else — including plain Return — writes one.
    if matches!(answered.as_deref(), None | Some("n" | "no")) {
        return Ok(None);
    }

    let Some(path) = ask(host, out, "Where to write it: ")?
        .map(|typed| PathBuf::from(typed.trim()))
        .filter(|path| !path.as_os_str().is_empty())
    else {
        // Not read as a change of mind. Somebody who has just asked for an
        // Export and then named nowhere to put it is likelier to have hit the
        // wrong key than to have decided, between two prompts, to give up every
        // Credential on the machine without a copy — and the way back is one
        // keystroke shorter than the way forward would have been.
        return Err(PerchError::Invalid(
            "No path was typed, so no Export was written.\n\
             Nothing was purged. Run `perch purge` again — answering `n` to the \
             offer purges without one."
                .to_string(),
        ));
    };
    refuse_a_path_the_purge_would_take(&path, home)?;

    export::write_the_export(host, perch, registry, &path, out)?;
    Ok(Some(path))
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
fn report(out: &mut dyn Write, home: &Path, purged: &Purged) -> Result<()> {
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
                "{} of them had nothing in either Credential Store to delete — on \
                 macOS a keychain item is filed under `$USER`, so one written \
                 under a different login name is still there.",
                purged.accounts - purged.credentials,
            ),
        )?;
    }

    say(
        out,
        "Claude Code is still logged in as whatever it was: the live Credential \
         was not Perch's to take away. `perch import` puts an Export back on a \
         machine like this one.",
    )
}
