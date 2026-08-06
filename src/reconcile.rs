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
//!   holds except the Credential and the file naming the Account, enumerated at
//!   Run time rather than listed in Perch's source — so a directory a Claude
//!   Code release invents follows the user without waiting for a Perch release.
//! - **It crosses by link, never by copy.** A copy diverges the moment it is
//!   edited, which inverts the one thing Shared State promises. Where no link
//!   can be made the Run is refused, naming the entry and the reason, because a
//!   refusal is recoverable and a silently diverged copy of somebody's memory
//!   is not.

use std::path::{Path, PathBuf};

use crate::error::{PerchError, Result};
use crate::host::{Host, HostError, Link, Platform};
use crate::probe;

/// The entries that stay behind: the Credential itself, and the file holding
/// `oauthAccount`. Both are the Account rather than the person, and both are
/// what a Profile exists to keep apart.
pub const ACCOUNT_SCOPED: [&str; 2] = [probe::CREDENTIALS_FILE, probe::IDENTITY_FILE];

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
    // Closed rather than at the umask, because a Profile holds a Credential
    // (ADR 0020).
    if !host.path_exists(into) {
        host.create_private_dir_all(into).map_err(|err| {
            PerchError::Other(format!("could not create {}: {err}", into.display()))
        })?;
    }

    for entry in crossing(host, shared)? {
        // The Profile itself, for the machine whose `CLAUDE_CONFIG_DIR` puts
        // one inside the other, and anything a listing yields that has no final
        // component to be named after. Both would be a link to itself, which is
        // a shape nothing recovers from.
        let Some(name) = entry.file_name().filter(|_| entry != *into) else {
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
            .filter(|entry| !account_scoped(entry))
            .collect()),
        Err(HostError::NotFound { .. }) => Ok(Vec::new()),
        Err(err) => Err(PerchError::file_read(shared.to_path_buf(), err)),
    }
}

/// Whether an entry is one of the two that stay behind.
///
/// Without regard to case, because Windows answers to `.Credentials.json` for
/// the same file — and the cost of the two answers is not the same: an entry
/// wrongly held back is one piece of Shared State a person has to notice
/// missing, and an entry wrongly crossed is a Credential in somebody else's
/// Profile.
fn account_scoped(entry: &Path) -> bool {
    entry
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            ACCOUNT_SCOPED
                .iter()
                .any(|held_back| name.eq_ignore_ascii_case(held_back))
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
        Err(err) => Err(refused(host, target, at, &err.to_string())),
    }
}

/// What to do when something that is not a link is where the share belongs.
///
/// On Windows that is the ordinary case rather than an obstruction: a shared
/// file there is routinely a hard link, which is a second name for the file and
/// says nothing about being one. It is re-established rather than trusted, for
/// a reason beyond not being able to recognise it — a hard link goes stale
/// silently. An editor that writes beside a file and renames it into place
/// leaves the Default Profile with a new file and the Profile still holding the
/// old one, which is the divergence this whole module exists to prevent. Doing
/// it before every Run is what keeps the window down to one Run.
///
/// Everywhere else a link would have been a symbolic one, so a plain file or a
/// real directory is something Perch did not put there. It is refused rather
/// than deleted: Perch does not know what it is, and deleting the thing you
/// cannot identify is how somebody loses work.
fn in_the_way(host: &dyn Host, target: &Path, at: &Path) -> Result<()> {
    if host.platform() == Platform::Windows && host.is_file(at) && host.is_file(target) {
        host.remove_file(at)
            .map_err(|err| refused(host, target, at, &err.to_string()))?;
        return make(host, target, at);
    }

    Err(refused(
        host,
        target,
        at,
        &format!(
            "{} is already there and is not a link Perch made",
            at.display()
        ),
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
    Err(refused(host, target, at, &refusals.join(", and ")))
}

/// Takes a link away, and says so in the same terms as a link that could not be
/// made: either way the entry is not reachable and the Run is not happening.
fn unlink(host: &dyn Host, at: &Path) -> Result<()> {
    host.remove_link(at).map_err(|err| {
        PerchError::Other(format!(
            "The link at {} could not be replaced: {err}\n\n\
             Perch shares by linking and never by copying (ADR 0026), so a share \
             it cannot repair refuses the Run rather than being worked around.",
            at.display()
        ))
    })
}

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
        Err(_) => return Ok(()),
    };

    for at in held {
        let Ok(Some(points_at)) = host.link_target(&at) else {
            continue;
        };
        if plain(&points_at).starts_with(plain(shared)) && !host.path_exists(&at) {
            unlink(host, &at)?;
        }
    }
    Ok(())
}

/// Why the Run is not happening, in the two pieces a person needs: which entry,
/// and what stopped it.
///
/// The fix is theirs rather than Perch's — a privilege to turn on, or a Profile
/// to move — so the message names it. Copying instead is not on the list.
fn refused(host: &dyn Host, target: &Path, at: &Path, why: &str) -> PerchError {
    let entry = target
        .file_name()
        .unwrap_or(target.as_os_str())
        .to_string_lossy()
        .into_owned();

    let fix = if host.platform() == Platform::Windows {
        "Turning on Developer Mode allows symbolic links; a Profile on a \
         filesystem that carries no links at all has to be moved to one that does."
    } else {
        "A Profile on a filesystem that carries no links has to be moved to one \
         that does."
    };

    PerchError::Other(format!(
        "`{entry}` could not be made reachable from {}: {why}.\n\n\
         Perch shares by linking and never by copying, because a copy diverges the \
         moment it is edited (ADR 0026) — so the Run is refused rather than served \
         one. {fix}",
        at.parent().unwrap_or(at).display(),
    ))
}

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

    #[test]
    fn the_two_account_scoped_entries_are_the_only_ones_held_back() {
        assert!(account_scoped(Path::new(
            "/Users/someone/.claude/.credentials.json"
        )));
        assert!(account_scoped(Path::new(
            "/Users/someone/.claude/.claude.json"
        )));
        assert!(!account_scoped(Path::new("/Users/someone/.claude/plugins")));
        assert!(!account_scoped(Path::new(
            "/Users/someone/.claude/CLAUDE.md"
        )));
    }

    /// A Windows filesystem answers to any spelling of a name, so a Credential
    /// must not be able to cross by being written in a different case.
    #[test]
    fn the_credential_stays_behind_however_it_is_spelled() {
        assert!(account_scoped(Path::new(
            "C:/Users/someone/.claude/.Credentials.JSON"
        )));
        assert!(account_scoped(Path::new(
            "C:/Users/someone/.claude/.Claude.json"
        )));
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
