//! Copying the handful of `.claude.json` keys that belong to the person into
//! the Profile a Run is about to launch (ADR everything-but-the-account).
//!
//! Everything else in a configuration directory crosses by link
//! ([`crate::reconcile`]). This one file cannot, because it holds
//! `oauthAccount` as well, so it is handled key by key and by a **named set**:
//! what crosses is listed rather than inverted.
//!
//! Nothing here is load-bearing. A key that goes missing costs a dialog rather
//! than correctness, which is why every reason to do nothing is a quiet one.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::host::{self, Host};
use crate::json;
use crate::probe::{self, Store};
use crate::registry::{Account, Registry};
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

/// Copies what belongs to the person into the Profile a Run is about to launch,
/// or leaves it exactly as it is. `default_profile` is where the active
/// Account's state lives, which is not that Account's Profile
/// (ADR claude-code-chooses-the-store). Returns nothing, because no reason to
/// do nothing here is worth stopping a Run for.
pub fn carry(
    host: &dyn Host,
    registry: &Registry,
    email: &str,
    default_profile: &Store,
    into: &Path,
    settled: Option<&crate::switch::Settled>,
) {
    // During a Landing no Account is active
    // (ADR a-switch-is-written-down-first), so `is_active` answers with the one
    // being *left* and its state would be looked for in the wrong directory.
    if settled.is_none() {
        return;
    }
    let destination = probe::identity_file_in(into);
    let Some(mine) = read(host, &destination) else {
        return;
    };

    // Discounting this process: the Run has already claimed the Profile by the
    // time it Carries, and read as an ordinary client that claim would decline
    // every Carry there is.
    if probe::anything_running_but(host, into, Some(host.process_id())) {
        return;
    }

    let Some(source) = most_recently_used(host, registry, email, default_profile, &destination)
    else {
        return;
    };
    let Some(theirs) = read(host, &source) else {
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
            "{} could not be written ({err}), so this Account starts as though \
             Claude Code had never run for you. The client was launched anyway.",
            destination.display()
        ));
    }
}

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

/// The identity file a Run copies from: the most recently used one holding
/// state that may cross into this Profile. Never the Profile being launched,
/// which is written by every Run and would outrank the person's own directory
/// from the first Run onwards, freezing what crosses there.
fn most_recently_used(
    host: &dyn Host,
    registry: &Registry,
    email: &str,
    default_profile: &Store,
    destination: &Path,
) -> Option<PathBuf> {
    let group = registry
        .account(email)
        .and_then(|account| account.group.clone());

    // `~/.claude.json` is the Default Profile's identity file wherever its config
    // directory is, so a link at a Profile's own hands one file two names — and
    // this one is written by every Run. Settled once for the many it is asked of.
    let launching = host::settled(host, destination);
    // Beside the resolved question rather than instead of it: a path that will
    // not settle is nowhere, so the spelling is what answers for a loop of links.
    let is_the_destination = |candidate: &Candidate| {
        candidate.identity_file == *destination
            || host::settled(host, &candidate.identity_file).is_the_same_place_as(&launching)
    };

    let mut candidates: Vec<Candidate> = registry
        .accounts
        .iter()
        .filter(|account| may_cross(account, email, group.as_deref()))
        .filter_map(|account| where_it_works(host, registry, account, default_profile))
        .filter(|candidate| !is_the_destination(candidate))
        .collect();

    // Most recently written first, and the directory the person actually works
    // in ahead of a Profile written at the same instant. A file that will not
    // say when is still a source, and the last one reached for.
    candidates.sort_by(|a, b| b.used.cmp(&a.used).then(b.is_default.cmp(&a.is_default)));
    candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.identity_file)
}

/// One place state might be copied from, and what decides between them.
struct Candidate {
    identity_file: PathBuf,
    /// Whether this is the Default Profile — the directory somebody is working
    /// in rather than one Perch is keeping.
    is_default: bool,
    /// When the Account it belongs to was last used. A file that will not say
    /// is a candidate that claims nothing.
    used: Option<DateTime<Utc>>,
}

/// Whether an Account's state may cross into the Profile being launched.
///
/// `same_name` rather than `==`, for both halves and for the reason
/// `Registry::is_active` gives: `upsert` matches case-folded and stores the
/// incoming spelling, so the Alias map can name an entry the old way.
fn may_cross(account: &Account, email: &str, group: Option<&str>) -> bool {
    use crate::registry::same_name;

    same_name(account.email(), email)
        || match (account.group.as_deref(), group) {
            (Some(held), Some(of)) => same_name(held, of),
            _ => false,
        }
}

/// Where an Account's `.claude.json` actually is: the Default Profile for the
/// active one, and its own Profile for every other.
fn where_it_works(
    host: &dyn Host,
    registry: &Registry,
    account: &Account,
    default_profile: &Store,
) -> Option<Candidate> {
    let active = registry.is_active(account.email());
    let identity_file = if active {
        default_profile.identity_file.clone()
    } else {
        probe::identity_file_in(&account.profile_dir(host).ok()?)
    };
    Some(Candidate {
        used: host.modified_at(&identity_file).ok(),
        identity_file,
        is_default: active,
    })
}

/// A file, where there is one to read. Not there, or unreadable, is nothing to
/// copy from and nothing to copy into — the ordinary state of a machine.
fn read(host: &dyn Host, path: &Path) -> Option<Zeroizing<String>> {
    host.read_file(path).ok().map(Zeroizing::new)
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

    #[test]
    fn an_account_spelled_the_other_way_is_still_itself() {
        let mut account = crate::registry::Account {
            identity: crate::probe::Identity {
                email: "Someone@Example.com".to_string(),
                account_uuid: None,
                organization_name: None,
                organization_uuid: None,
            },
            plan: None,
            disabled: false,
            quarantine: None,
            group: None,
            utilization: None,
        };

        assert!(
            may_cross(&account, "someone@example.com", None),
            "the Account being launched may always carry from itself"
        );

        account.group = Some("work".to_string());
        assert!(
            may_cross(&account, "nobody@example.com", Some("work")),
            "and a Group still crosses on the Group"
        );
        assert!(
            may_cross(&account, "nobody@example.com", Some("Work")),
            "however it was capitalized, which is what `same_name` is for and \
             what every other Group comparison in the registry already does"
        );
        assert!(
            !may_cross(&account, "nobody@example.com", None),
            "while an unrelated Account in no Group crosses nothing"
        );
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
