//! Making every piece of Shared State reachable from the Profile a Run is
//! about to launch (ADR everything-but-the-account).
//!
//! A Run is the one path where a Profile is a live configuration directory
//! rather than storage (ADR a-run-is-one-shot), so it is the one path that has
//! to do this. A Switch leaves the Default Profile where it is, and Shared
//! State follows the person by never having moved.
//!
//! Two rules, and everything here is one of them: what crosses is decided by a
//! denylist read at Run time, and it crosses by link or the Run is refused.

use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{self, Host, HostError, Link, Platform};
use crate::{probe, profile};

/// The entries that stay behind, for the two reasons there are to hold one
/// back: the first two are the Account rather than the person, and the last two
/// answer a question about *this* configuration directory, which means nothing
/// in another one. `.oauth_refresh.lock` is the only one of Claude Code's three
/// locks that sits inside the directory, so the only one a Reconcile sees.
pub const HELD_BACK: [&str; 4] = [
    probe::CREDENTIALS_FILE,
    probe::IDENTITY_FILE,
    probe::SESSIONS,
    probe::REFRESH_LOCK,
];

/// Makes every piece of Shared State in `shared` reachable from `into`,
/// repairing whatever share is there already, or says why it cannot.
///
/// A no-op on the second pass over an unchanged machine, apart from the hard
/// links, which cannot be told from the files they name (see [`in_the_way`]).
pub fn reconcile(host: &dyn Host, shared: &Path, into: &Path) -> Result<()> {
    // A Profile that *is* the Default Profile already holds all of it, and the
    // loop below has no guard for it: that one asks whether an entry *holds* the
    // Profile, and a Profile linked at the Default Profile holds none of them.
    if host::is_the_same_place(host, shared, into) {
        return Ok(());
    }

    // A link is made inside a directory, so the directory has to be there.
    if !host.path_exists(into) {
        profile::make_dir(host, into)?;
    }

    // Both sides resolved, which is what *is this inside that* takes: a link
    // above the Profile makes one directory with two spellings, and a share
    // through the other one links the Profile into a directory holding it.
    let here = host::settled(host, into);
    for entry in crossing(host, shared)? {
        let holds_the_profile = here.is_inside(&host::settled(host, &entry));
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
/// beside one of them: `.perch-tmp.<pid>`, `.perch-takeover`, and Claude Code's
/// own `.claude.json.lock`. By prefix and without regard to case, so a
/// `sessions.md` somebody wrote is held back too — which is the side of this to
/// be wrong on, one missing file against a Credential in another Profile.
fn held_back(entry: &Path) -> bool {
    entry
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            // `get` rather than a slice: a name is arbitrary text and the
            // prefix length is a byte count that need not land on a character
            // boundary.
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

/// What to do when something that is not a link is where the share belongs: on
/// Windows the ordinary case, because a share there is routinely a hard link and
/// nothing about a second name says it is one; everywhere else something Perch
/// did not put there, refused rather than deleted. The asymmetry between the
/// two cannot be closed.
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

/// Makes the link this platform can make for this kind of entry. The hard-link
/// fallback is Windows' alone, because the re-establishment that keeps one
/// honest is (see [`in_the_way`]): a machine falling back anywhere else would
/// refuse its own share on the next pass. Nothing is given up — a filesystem
/// carrying hard links and no symbolic links is a Windows one.
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

/// Takes a link away, reported the way an unmakeable link is. The remedy is its
/// own, though: a link that will not *go* is the permissions on the directory
/// holding it — what a `sudo claude` leaves behind — so advising Developer Mode
/// there is the wrong remedy `refused` would rather name none.
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
/// anything: a dangling link is not inert, because Claude Code takes its locks
/// by `mkdir` (ADR claude-code-chooses-the-store) and `mkdir` at one fails as
/// though the lock were held. Only links, only into the Default Profile, and
/// only at something gone — everything else in a Profile is somebody's.
fn sweep(host: &dyn Host, shared: &Path, into: &Path) -> Result<()> {
    let held = match host.list_dir(into) {
        Ok(held) => held,
        // A Profile that will not say what it holds is a different answer from
        // one that is not there, and swallowing it would leave the dangling
        // links this exists to clear.
        Err(HostError::NotFound { .. }) => return Ok(()),
        Err(err) => return Err(PerchError::file_read(into.to_path_buf(), err)),
    };

    for at in held {
        let Ok(Some(points_at)) = host.link_target(&at) else {
            continue;
        };
        // Wherever it points, and asked before the target is: a Profile's
        // Credential Store is derived from its directory, so a link at one of
        // these names is an Account holding a Credential that is not its own.
        if held_back(&at) {
            unlink(host, &at)?;
            continue;
        }
        // The target as it was recorded, unresolved: this asks what the link
        // says rather than where it lands, and a target that is gone — which is
        // the case this sweep is for — resolves to nothing at all.
        #[allow(
            clippy::disallowed_methods,
            reason = "what a link records, which is text and not a place"
        )]
        let into_the_shared_state = plain(&points_at).starts_with(plain(shared));
        if into_the_shared_state && !host.path_exists(&at) {
            unlink(host, &at)?;
        }
    }
    Ok(())
}

/// Why the Run is not happening, in the three pieces a person needs: which
/// entry, what stopped it, and what to do about it. The remedy is passed in
/// because the two failures have nothing in common — a privilege or a filesystem
/// against a path to move — and naming the wrong one is worse than naming none.
fn refused(at: &Path, why: &str, remedy: &str) -> PerchError {
    let entry = at
        .file_name()
        .unwrap_or(at.as_os_str())
        .to_string_lossy()
        .into_owned();

    PerchError::Other(format!(
        "`{entry}` could not be made reachable from {}: {why}.\n\n\
         Perch shares by linking and never by copying, because a copy diverges \
         the moment it is edited — so the Run is refused rather than served \
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

    /// Driven off `HELD_BACK` rather than off a list beside it, which could
    /// name three of the four and never notice.
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

    /// The dangerous one is the temp file: every atomic write puts one holding
    /// the whole of its target beside it, so a Run starting while a Switch is
    /// mid-write meets a complete copy of `.credentials.json`.
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
