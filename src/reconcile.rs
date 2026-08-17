//! Making every piece of Shared State reachable from the Profile a Run is
//! about to launch (ADR 0026).
//!
//! A Run is the one path where a Profile is a live configuration directory
//! rather than storage (ADR 0010), so it is the one path that has to do this. A
//! Switch does not: it leaves the Default Profile where it is, and Shared State
//! follows the person by never having moved.
//!
//! Two rules, and everything here is one of them:
//!
//! - **What crosses is decided by denylist.** Everything the Default Profile
//!   holds except the Credential, the file naming the Account and the directory
//!   of session markers, enumerated at Run time rather than listed in Perch's
//!   source — so a directory a Claude Code release invents follows the user
//!   without waiting for a Perch release.
//! - **It crosses by link, never by copy.** A copy diverges the moment it is
//!   edited, which inverts the one thing Shared State promises. Where no link
//!   can be made the Run is refused, naming the entry and the reason, because a
//!   refusal is recoverable and a silently diverged copy of somebody's memory
//!   is not.

use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{self, Host, HostError, Link, Platform};
use crate::{probe, profile};

/// The entries that stay behind, for the two different reasons there are to
/// hold one back.
///
/// `.credentials.json` and `.claude.json` are the Account rather than the
/// person, and keeping them apart is what a Profile is for.
///
/// `sessions` is neither: it is the config directory's own answer to "is a
/// client running here". Shared, every Profile would report every other
/// Profile's clients and its own Run's marker would land in the Default
/// Profile — so one client would make every Profile Live at once, and the
/// refusals that protect a Live Profile would fire for every Account on the
/// machine (ADR 0027).
///
/// `.oauth_refresh.lock` is the directory's too, and is the same class of
/// mistake with a worse ending. It is the only one of Claude Code's three locks
/// that sits *inside* the config directory rather than beside it, so it is the
/// only one a Reconcile ever sees — and it is only there at all while somebody
/// is holding it, which is what made this rare enough to ship. A `perch run`
/// starting while a `perch switch` held it linked the Profile's lock to the
/// Default Profile's; the Switch then finished and removed the real directory,
/// leaving a link to nothing. `mkdir` at a dangling link fails exactly as it
/// does when the lock is held, so that Profile's client waits on a lock nobody
/// holds — until `clear_the_abandoned` takes the link away, which it can,
/// because a dangling link has no modification time to read as a hold. Live
/// rather than dangling is worse, and is the case with no way out: two
/// configuration directories sharing one lock, each reading the other's mtime
/// as its own.
///
/// So the denylist ADR 0026 wrote as two entries is four, and the last two are
/// one rule — an entry that answers a question about *this* directory means
/// nothing in another one.
pub const HELD_BACK: [&str; 4] = [
    probe::CREDENTIALS_FILE,
    probe::IDENTITY_FILE,
    probe::SESSIONS,
    probe::REFRESH_LOCK,
];

/// Makes every piece of Shared State in `shared` reachable from `into`,
/// repairing whatever share is there already, or says why it cannot.
///
/// Runs before every Run, and is a no-op on the second pass over an unchanged
/// machine — apart from the hard links, which cannot be told from the files
/// they name and are therefore re-established every time (see [`in_the_way`]).
pub fn reconcile(host: &dyn Host, shared: &Path, into: &Path) -> Result<()> {
    // A Profile that *is* the Default Profile already holds all of it. Nothing
    // below would survive being pointed at itself, and this is cheaper than
    // finding out one entry at a time.
    if shared == into {
        return Ok(());
    }

    // A link is made inside a directory, so the directory has to be there.
    if !host.path_exists(into) {
        profile::make_dir(host, into)?;
    }

    for entry in crossing(host, shared)? {
        // The Profile itself, for the machine whose `CLAUDE_CONFIG_DIR` puts
        // one inside the other, and anything a listing yields that has no final
        // component to be named after. Both would be a link to itself, which is
        // a shape nothing recovers from.
        //
        // Anything *containing* the Profile, not only the Profile exactly:
        // `PERCH_HOME=~/.claude/perch` under `CLAUDE_CONFIG_DIR=~/.claude` makes
        // the crossing entry `~/.claude/perch` and the Profile several levels
        // below it. Compared for equality the two are different paths, so the
        // link was made — and its subtree held the Profile it was made in, so
        // every walk through that Profile afterwards recurses without bottom.
        // And anything that *resolves* to somewhere containing the Profile, not
        // only anything spelled that way. A `~/.claude/perch` that is a link to
        // `~/.config/perch` passes the textual test — the two are different
        // strings — and linking it into a Profile under `~/.config/perch` makes
        // the same bottomless subtree by a route the comparison could not see.
        // One hop is what a dotfile manager makes, and what `through_any_link`
        // already resolves for the other reader of somebody else's links.
        let holds_the_profile =
            into.starts_with(&entry) || into.starts_with(host::through_any_link(host, &entry));
        let Some(name) = entry.file_name().filter(|_| !holds_the_profile) else {
            continue;
        };
        establish(host, &entry, &into.join(name))?;
    }
    sweep(host, shared, into)
}

/// What the Default Profile holds that belongs to the person, read now rather
/// than believed from a list.
///
/// A Default Profile that is not there shares nothing, which is a machine
/// Claude Code has never run on rather than a failure.
fn crossing(host: &dyn Host, shared: &Path) -> Result<Vec<PathBuf>> {
    match host.list_dir(shared) {
        Ok(entries) => Ok(entries
            .into_iter()
            .filter(|entry| !held_back(entry))
            .collect()),
        Err(HostError::NotFound { .. }) => Ok(Vec::new()),
        Err(err) => Err(PerchError::file_read(shared.to_path_buf(), err)),
    }
}

/// Whether an entry is one of the four that stay behind, or something written
/// beside one of them.
///
/// Without regard to case, because Windows answers to `.Credentials.json` for
/// the same file — and the cost of the two answers is not the same: an entry
/// wrongly held back is one piece of Shared State a person has to notice
/// missing, and an entry wrongly crossed is a Credential in somebody else's
/// Profile.
///
/// And by prefix, because what is written beside a held-back name is the same
/// thing under a longer one. `write_atomically` and every private write put a
/// `.perch-tmp.<pid>` beside their target, holding the *whole* of it — so a Run
/// starting while a Switch was mid-write would cross a complete copy of
/// `.credentials.json` into another Account's Profile. `lock::take_over` puts a
/// `.perch-takeover` beside `.oauth_refresh.lock`, which is an answer about one
/// configuration directory exactly as the lock is. And Claude Code's own
/// config-file lock is `.claude.json.lock`.
///
/// A name that merely begins the same way and belongs to somebody — a
/// `sessions.md` — is held back too. That is the side of this to be wrong on:
/// one file a person notices missing from a Run against the alternative.
fn held_back(entry: &Path) -> bool {
    entry
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            // `get` rather than a slice: a name is arbitrary text, and the
            // prefix length is a byte count that need not land on a character
            // boundary. `None` there is simply a name that does not begin with
            // this one.
            HELD_BACK.iter().any(|stays| {
                name.get(..stays.len())
                    .is_some_and(|begins| begins.eq_ignore_ascii_case(stays))
            })
        })
}

/// Makes one entry reachable, whatever is at the other end already.
fn establish(host: &dyn Host, target: &Path, at: &Path) -> Result<()> {
    match host.link_target(at) {
        // Already the share it should be. Left exactly as it is: re-making a
        // link that is right is a window in which the entry is not there.
        Ok(Some(points_at)) if points_to(&points_at, target) => Ok(()),
        // A link, but not to here — the Profile moved, or the entry did.
        // Removing a link costs nothing and what it pointed at is untouched.
        Ok(Some(_)) => {
            unlink(host, at)?;
            make(host, target, at)
        }
        Ok(None) => in_the_way(host, target, at),
        Err(HostError::NotFound { .. }) => make(host, target, at),
        Err(err) => Err(refused(at, &err.to_string(), no_link_here(host))),
    }
}

/// What to do when something that is not a link is where the share belongs.
///
/// On Windows that is the ordinary case rather than an obstruction: a shared
/// file there is routinely a hard link, which is a second name for the file and
/// says nothing about being one. It is re-established rather than trusted, for
/// a reason beyond not being able to recognize it — a hard link goes stale
/// silently. An editor that writes beside a file and renames it into place
/// leaves the Default Profile with a new file and the Profile still holding the
/// old one, which is the divergence this whole module exists to prevent. Doing
/// it before every Run is what keeps the window down to one Run.
///
/// Everywhere else a share is a symbolic link and says so, so a plain file or a
/// real directory is something Perch did not put there — a file the client of
/// an earlier Run wrote before the Default Profile had an entry of that name,
/// most likely. It is refused rather than deleted, because deleting the thing
/// you cannot identify is how somebody loses work, and the refusal names the
/// path so the fix is one command.
///
/// So Windows deletes a file it cannot identify, where every other platform
/// refuses to — which reads as the exception that should be closed, and cannot
/// be. The obvious guard is that a hard link is a second name for one file and
/// therefore shares its modification time, so a file whose mtime is anything
/// else was not made here. But the file this branch exists to delete is
/// *precisely* that one: an editor that writes beside the target and renames it
/// into place leaves the Default Profile holding a new file while the Profile
/// still names the old one, and repairing that divergence is the whole reason a
/// share is re-established before every Run. Every stale hard link Perch made
/// fails the test, so the guard would refuse exactly the case it was added to
/// protect and leave the divergence in place.
///
/// Nothing weaker works either, because nothing about a second name says it is
/// one. What would work is Perch recording which entries it linked, which is a
/// file of its own in the Profile with its own ways to be wrong — a record lost
/// refuses shares Perch did make, and a record kept past a removal deletes a
/// file somebody put there afterwards. It has not been judged worth that.
fn in_the_way(host: &dyn Host, target: &Path, at: &Path) -> Result<()> {
    if host.platform() == Platform::Windows && host.is_file(at) && host.is_file(target) {
        host.remove_file(at)
            .map_err(|err| refused(at, &err.to_string(), MOVE_IT_ASIDE))?;
        return make(host, target, at);
    }

    Err(refused(
        at,
        &format!(
            "{} is already there and is not a link Perch made",
            at.display()
        ),
        MOVE_IT_ASIDE,
    ))
}

/// Makes the link this platform can make for this kind of entry.
///
/// Directories are junctions on Windows and symbolic links everywhere else:
/// only symbolic links need Developer Mode or elevation, and a junction needs
/// neither. Files try a symbolic link first even on Windows — where the
/// privilege is there it is the better share, because it survives the file it
/// names being replaced — and fall back to a hard link, which is what is left
/// when it is not.
///
/// The fallback is Windows' alone, because the re-establishment that keeps a
/// hard link honest is Windows' alone (see [`in_the_way`]): a machine that fell
/// back anywhere else would hold a share Perch could not tell from a stray file
/// and would refuse on the next pass, which is a worse answer than refusing
/// now. Nothing is given up by that. A filesystem carrying hard links and no
/// symbolic links is a Windows one; every unix filesystem that has the first
/// has the second.
fn make(host: &dyn Host, target: &Path, at: &Path) -> Result<()> {
    let windows = host.platform() == Platform::Windows;
    let kinds: &[Link] = match (host.is_file(target), windows) {
        (false, true) => &[Link::Junction],
        (false, false) => &[Link::Symbolic],
        (true, true) => &[Link::Symbolic, Link::Hard],
        (true, false) => &[Link::Symbolic],
    };

    let mut refusals = Vec::new();
    for kind in kinds {
        match host.link(*kind, target, at) {
            Ok(()) => return Ok(()),
            Err(err) => refusals.push(format!("{} could not be made ({err})", kind.describe())),
        }
    }
    Err(refused(at, &refusals.join(", and "), no_link_here(host)))
}

/// Takes a link away, and reports a failure the way an unmakeable link is
/// reported: either way the entry is not reachable and the Run is not
/// happening.
///
/// The remedy is its own, though, and not `no_link_here`. A link that will not
/// *go* is nothing to do with whether this machine can make one — it is the
/// permissions on the directory holding it, which is what a `sudo claude` that
/// left a Profile root-owned produces. Advising Developer Mode there is the
/// wrong-remedy case `refused` says would be worse than naming none.
fn unlink(host: &dyn Host, at: &Path) -> Result<()> {
    host.remove_link(at).map_err(|err| {
        refused(
            at,
            &format!("the link that shares it could not be replaced ({err})"),
            WILL_NOT_GO,
        )
    })
}

/// What to do about a link that will not go: the directory it sits in is the
/// thing refusing, and its permissions are the user's.
const WILL_NOT_GO: &str = "That link is Perch's own and is being replaced, so what refused is the directory holding \
     it — check that you own it and can write to it.";

/// Clears away links into the Default Profile that no longer stand for
/// anything.
///
/// An entry deleted from the Default Profile is not enumerated, so nothing else
/// here would ever look at the link that used to share it — and a link to
/// nothing is not inert. Claude Code takes its locks by `mkdir` (ADR 0001), and
/// `mkdir` at a dangling link fails exactly as it does when the lock is held,
/// so one left behind is a Profile whose client waits for a lock nobody holds.
///
/// Only links, only ones pointing into the Default Profile, and only ones
/// pointing at something that has gone. Everything else in a Profile is
/// somebody's, and this is a repair rather than a tidy-up.
fn sweep(host: &dyn Host, shared: &Path, into: &Path) -> Result<()> {
    let held = match host.list_dir(into) {
        Ok(held) => held,
        // A Profile that is not there holds no links to sweep. A Profile that
        // will not say what it holds is a different answer, and swallowing it
        // would leave the dangling links this exists to clear — so it is
        // reported rather than passed over, as the enumeration above is.
        Err(HostError::NotFound { .. }) => return Ok(()),
        Err(err) => return Err(PerchError::file_read(into.to_path_buf(), err)),
    };

    for at in held {
        let Ok(Some(points_at)) = host.link_target(&at) else {
            continue;
        };
        if !plain(&points_at).starts_with(plain(shared)) {
            continue;
        }
        // A link at a held-back name goes whether or not its target is still
        // there. The denylist is enforced where links are *made* — those names
        // are filtered out before anything is established — so nothing here
        // would ever look at one sitting at `.credentials.json` or `sessions`
        // again, and it would stay for good. What it means is not a tidiness
        // question: a Profile whose Credential Store is a link into the Default
        // Profile has no Credential of its own, so a Capture or a relogin
        // writing into it writes into the live store; and a `sessions` link
        // makes every Profile Live at once (ADR 0027).
        if held_back(&at) || !host.path_exists(&at) {
            unlink(host, &at)?;
        }
    }
    Ok(())
}

/// Why the Run is not happening, in the three pieces a person needs: which
/// entry, what stopped it, and what to do about it.
///
/// The remedy is passed in rather than composed here, because the two failures
/// have nothing in common: a link this machine will not make is a privilege or
/// a filesystem, and something sitting where the share belongs is a path to
/// move. A refusal that named the wrong one would be worse than one that named
/// none. Copying instead is on neither list.
fn refused(at: &Path, why: &str, remedy: &str) -> PerchError {
    let entry = at
        .file_name()
        .unwrap_or(at.as_os_str())
        .to_string_lossy()
        .into_owned();

    PerchError::Other(format!(
        "`{entry}` could not be made reachable from {}: {why}.\n\n\
         Perch shares by linking and never by copying, because a copy diverges the \
         moment it is edited (ADR 0026) — so the Run is refused rather than served \
         one. {remedy}",
        at.parent().unwrap_or(at).display(),
    ))
}

/// What to do about a link this machine will not make. Both halves of it are
/// the user's: Perch can no more grant itself a privilege than move a Profile
/// onto another filesystem.
fn no_link_here(host: &dyn Host) -> &'static str {
    if host.platform() == Platform::Windows {
        "Turning on Developer Mode allows symbolic links; a Profile on a \
         filesystem that carries no links at all has to be moved to one that does."
    } else {
        "A Profile on a filesystem that carries no links has to be moved to one \
         that does."
    }
}

/// What to do about something sitting where a share belongs. Naming the act
/// matters: without it this reads as a Run that can never happen again, and it
/// is one `mv` from happening.
const MOVE_IT_ASIDE: &str = "Whatever is at that path is not Perch's to delete, so move it aside or \
     remove it yourself and run again.";

/// Whether a link stands for exactly this path.
///
/// Compared without the prefixes Windows spells a reparse point's target with:
/// a junction records `\??\C:\...` and reads back as one form or the other, and
/// a comparison that missed that would rebuild every junction on every Run.
fn points_to(recorded: &Path, target: &Path) -> bool {
    plain(recorded) == plain(target)
}

/// A path as it reads without a verbatim prefix. Nothing anywhere else spells
/// one, so this is Windows-only in effect and platform-free in fact.
fn plain(path: &Path) -> &Path {
    let Some(spelled) = path.to_str() else {
        return path;
    };
    for prefix in [r"\\?\", r"\??\"] {
        if let Some(rest) = spelled.strip_prefix(prefix) {
            return Path::new(rest);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Driven off `HELD_BACK` rather than off a list beside it. Spelled out,
    /// this loop named three of the four and had done since the entry that
    /// makes it four was added — so the denylist could lose `.oauth_refresh.lock`
    /// and the test that exists to say what is on it would not notice.
    #[test]
    fn the_entries_that_stay_behind_are_the_only_ones() {
        for stays in HELD_BACK {
            assert!(
                held_back(&Path::new("/Users/someone/.claude").join(stays)),
                "{stays} stays behind"
            );
        }
        for crosses in ["plugins", "CLAUDE.md", "plans", "session-env"] {
            assert!(
                !held_back(&Path::new("/Users/someone/.claude").join(crosses)),
                "{crosses} follows the person into a Run"
            );
        }
    }

    /// What is written *beside* a held-back name is the same thing under a
    /// longer one.
    ///
    /// The dangerous one is the temp file: every atomic write in Perch creates
    /// `<target>.perch-tmp.<pid>` holding the whole of the target and then
    /// renames it into place, so a Run starting while a Switch was mid-write
    /// enumerated a complete copy of `.credentials.json` and linked it into
    /// another Account's Profile. `.oauth_refresh.lock.perch-takeover` is the
    /// same class as the lock it guards — an answer about one configuration
    /// directory, meaningless in another — and `.claude.json.lock` is Claude
    /// Code's own.
    #[test]
    fn what_is_written_beside_something_that_stays_behind_stays_behind_too() {
        for beside in [
            ".credentials.json.perch-tmp.4242",
            ".claude.json.perch-tmp.4242",
            ".claude.json.lock",
            ".oauth_refresh.lock.perch-takeover",
            "sessions.perch-tmp.4242",
        ] {
            assert!(
                held_back(&Path::new("/Users/someone/.claude").join(beside)),
                "{beside} stays behind"
            );
        }
        for crosses in [".credential-notes.md", "claude.json", "session-env"] {
            assert!(
                !held_back(&Path::new("/Users/someone/.claude").join(crosses)),
                "{crosses} follows the person into a Run"
            );
        }
    }

    /// A Windows filesystem answers to any spelling of a name, so a Credential
    /// must not be able to cross by being written in a different case.
    #[test]
    fn what_stays_behind_stays_behind_however_it_is_spelled() {
        for spelling in [
            ".Credentials.JSON",
            ".Claude.json",
            "Sessions",
            ".OAuth_Refresh.LOCK",
        ] {
            assert!(
                held_back(&Path::new("C:/Users/someone/.claude").join(spelling)),
                "{spelling} is the same entry"
            );
        }
    }

    #[test]
    fn a_junctions_verbatim_target_is_the_path_it_names() {
        assert!(points_to(
            Path::new(r"\??\C:\Users\someone\.claude\plugins"),
            Path::new(r"C:\Users\someone\.claude\plugins")
        ));
        assert!(points_to(
            Path::new(r"\\?\C:\Users\someone\.claude\plugins"),
            Path::new(r"C:\Users\someone\.claude\plugins")
        ));
        assert!(!points_to(
            Path::new(r"C:\Users\someone\.claude\plans"),
            Path::new(r"C:\Users\someone\.claude\plugins")
        ));
    }
}
