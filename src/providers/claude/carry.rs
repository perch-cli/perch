//! Copying the handful of `.claude.json` keys that belong to the person into
//! the Profile a Run is about to launch (ADR everything-but-the-account).
//!
//! Everything else in a configuration directory crosses by link
//! ([`super::reconcile`]). This one file cannot, because it holds
//! `oauthAccount` as well, so it is handled key by key and by a **named set**:
//! what crosses is listed rather than inverted.
//!
//! Nothing here is load-bearing. A key that goes missing costs a dialog rather
//! than correctness, which is why every reason to do nothing is a quiet one.

use std::path::Path;

use crate::host::{self, Host};
use crate::json;
use crate::live;
use crate::providers::claude::probe;
use crate::secret::Secret;
use zeroize::Zeroizing;

/// The keys of `.claude.json` that belong to the person rather than to the
/// Account, and therefore cross into the Profile a Run launches: state Claude
/// Code accumulates about the person, whose absence costs a dialog rather than
/// correctness. Nothing keyed to the Account that filled it, however
/// person-shaped it looks. A thing to maintain, and short on purpose.
pub const PERSON_KEYS: [&str; 4] = [
    "hasCompletedOnboarding",
    "lastOnboardingVersion",
    "tipsHistory",
    "seenNotifications",
];

/// One entry per directory Claude Code has been run in: the trust it was given
/// there, the tools it was allowed, and that directory's MCP configuration.
const PROJECTS: &str = "projects";

/// The keys of one `projects` entry that cross, on the same terms as
/// [`PERSON_KEYS`]: what a person decided about this directory, and nothing an
/// Account filled in working there. Claude Code keeps its per-directory figures
/// here too — what the last session cost, how many tokens it spent, which
/// session it was — and a figure is read for one Account.
pub const PROJECT_KEYS: [&str; 7] = [
    "hasTrustDialogAccepted",
    "hasCompletedProjectOnboarding",
    "projectOnboardingSeenCount",
    "allowedTools",
    "mcpServers",
    "enabledMcpjsonServers",
    "disabledMcpjsonServers",
];

/// `mine` with every key that crosses taken from `theirs`, and every other byte
/// of it as it was — or nothing where no key crossed. Each key is a bounded read
/// and a bounded write of its own ([`crate::json`]), asked through
/// [`json::changed_value_at`], so a Profile already holding what would cross
/// costs no copy of the document at all.
fn crossed(host: &dyn Host, theirs: &str, mine: &str) -> Option<Secret> {
    let mut patched: Option<Secret> = None;
    for key in PERSON_KEYS {
        let Some(value) = json::value_at(theirs, key) else {
            continue;
        };
        let against = patched.as_deref().unwrap_or(mine);
        if let Some(written) = json::changed_value_at(against, key, value) {
            patched = Some(written);
        }
    }
    project_entry(host, theirs, patched, mine)
}

/// The same for `projects[<current working directory>]`, which is one entry of
/// one key rather than a key — and only that directory, because an Account does
/// not need the tool approvals of work it is not doing.
fn project_entry(
    host: &dyn Host,
    theirs: &str,
    patched: Option<Secret>,
    mine: &str,
) -> Option<Secret> {
    let Ok(here) = host.current_dir() else {
        return patched;
    };
    let here = here.to_string_lossy();

    let Some(theirs_here) =
        json::value_at(theirs, PROJECTS).and_then(|projects| json::value_at(projects, &here))
    else {
        return patched;
    };

    let against = patched.as_deref().unwrap_or(mine);
    // A Profile that has never been run holds no `projects` at all, which is an
    // empty one for this purpose, as is an entry it holds for no directory.
    let held = json::value_at(against, PROJECTS).unwrap_or("{}");
    let mine_here = Secret::copied(json::value_at(held, &here).unwrap_or("{}"));

    let mut entry: Option<Secret> = None;
    for key in PROJECT_KEYS {
        let Some(value) = json::value_at(theirs_here, key) else {
            continue;
        };
        let into = entry.as_deref().unwrap_or(&mine_here);
        if let Some(written) = json::changed_value_at(into, key, value) {
            entry = Some(written);
        }
    }
    // Nothing of the person's in their entry leaves this Profile's own alone,
    // rather than writing back a copy of what was already there.
    let Some(entry) = entry else {
        return patched;
    };

    let Some(projects) = json::changed_value_at(held, &here, &entry) else {
        return patched;
    };
    json::changed_value_at(against, PROJECTS, &projects).or(patched)
}

/// A file, where there is one to read. Not there, or unreadable, is nothing to
/// copy from and nothing to copy into — the ordinary state of a machine.
fn read(host: &dyn Host, path: &Path) -> Option<Zeroizing<String>> {
    host.read_file(path).ok().map(Zeroizing::new)
}

/// Source admission belongs to Group policy; native file selection stays here.
pub(crate) fn from_profiles(
    host: &dyn Host,
    profiles: &[crate::providers::provider::SharedProfile],
    into: &Path,
) {
    let destination = probe::identity_file_in(into);
    let target = host::settled(host, &destination);
    let mut candidates: Vec<_> = profiles
        .iter()
        .filter_map(|source| {
            let path = if source.is_default {
                crate::providers::claude::layout::default_profile(host)
                    .ok()?
                    .identity_file
            } else {
                probe::identity_file_in(&source.path)
            };
            if path == destination || host::settled(host, &path).is_the_same_place_as(&target) {
                return None;
            }
            Some((host.modified_at(&path).ok(), source.is_default, path))
        })
        .collect();
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let sources: Vec<_> = candidates.into_iter().map(|(_, _, path)| path).collect();
    // Discounting this process: the Run has already claimed the Profile by the
    // time it Carries, and read as an ordinary client that claim would decline
    // every Carry there is.
    if live::ask(
        host,
        &[live::Place::at(
            crate::providers::provider::Id::Claude,
            into,
        )],
    )
    .counts_as_live_but(Some(host.process_id()))
    {
        return;
    }

    let destination = probe::identity_file_in(into);
    let Some(mine) = read(host, &destination) else {
        return;
    };

    // The first that reads, rather than the best alone: a `sudo claude` leaves a
    // root-owned `.claude.json` that is newest by mtime, and giving up on it
    // costs a dialog on every Run for ever where the next candidate would serve.
    let Some(theirs) = sources.iter().find_map(|source| read(host, source)) else {
        return;
    };

    let Some(patched) = crossed(host, &theirs, &mine) else {
        return;
    };
    if patched.as_str() == mine.as_str() {
        return;
    }
    // A source truncated mid-token has no `,` or brace for `json::value_at` to
    // stop at, so the span handed back is `tru`. Asked of the result rather
    // than the source, because the result is what gets written.
    if serde_json::from_str::<serde::de::IgnoredAny>(&patched).is_err() {
        return;
    }
    // Said rather than swallowed, and said rather than raised: a remark naming
    // the file turns onboarding questions on every Run into a `chmod`.
    if let Err(err) = host::write_atomically(host, &destination, &patched) {
        host.note(&format!(
            "{} could not be written ({err}), so this Account starts without your \
             Claude Code settings.",
            destination.display()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of the argument for naming what crosses rather than inverting
    /// it: Perch is what would show one Account's figures under another
    /// Account's name (ADR a-figure-names-its-account).
    #[test]
    fn nothing_keyed_to_an_account_is_in_the_set() {
        for key in [
            "oauthAccount",
            "cachedUsageUtilization",
            "modelAccessCache",
            "overageCreditGrantCache",
            "orgModelDefaultCache",
        ] {
            assert!(!PERSON_KEYS.contains(&key), "{key} does not cross");
        }
    }

    /// Each of the three kinds of first-run friction, or the dialog the set
    /// exists to prevent comes back.
    #[test]
    fn onboarding_tips_and_notifications_are_all_covered() {
        for about in ["Onboarding", "tips", "Notifications"] {
            assert!(
                PERSON_KEYS
                    .iter()
                    .any(|key| key.to_lowercase().contains(&about.to_lowercase())),
                "nothing in the set is about {about}"
            );
        }
    }
}
