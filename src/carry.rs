//! Copying the handful of `.claude.json` keys that belong to the person into
//! the Profile a Run is about to launch (ADR 0003).
//!
//! Everything else in a configuration directory crosses by link and never by
//! copy ([`crate::reconcile`]). This one file cannot: it holds `oauthAccount`,
//! which is the Account itself, so a Profile has to keep its own. And it also
//! holds a great deal that is the person's — whether they have been through
//! onboarding, which tips they have seen, and the tools they approved for the
//! repository they are standing in. A Profile served none of that is a Claude
//! Code that believes it has never been run, asking for trust and permissions
//! in the middle of somebody's task.
//!
//! So the file is handled key by key, and by a **named set**: what crosses is
//! listed here rather than inverted. That is the opposite of the direction the
//! directory takes, and deliberately — `.claude.json` also holds answers
//! Anthropic gave for one Account, and carrying those across Accounts would
//! feed Perch's own Utilization figures that were read for somebody else.
//!
//! Nothing here is load-bearing. A key that goes missing costs a dialog rather
//! than correctness, which is why every reason to do nothing is a quiet one.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::host::{self, Host};
use crate::json;
use crate::probe::{self, Store};
use crate::registry::{Account, Registry};

/// The keys of `.claude.json` that belong to the person rather than to the
/// Account, and therefore cross into the Profile a Run launches.
///
/// What qualifies: state Claude Code accumulates about the person using it,
/// whose absence costs them a dialog or a first-run prompt rather than
/// costing correctness. Onboarding, tips and notifications are the whole of
/// it today.
///
/// What does not, however person-shaped it looks: anything keyed to the
/// Account that filled it. `oauthAccount` names the Account, and
/// `cachedUsageUtilization`, `modelAccessCache`, `overageCreditGrantCache` and
/// `orgModelDefaultCache` are answers Anthropic gave about one subscription —
/// carrying those would put one Account's figures in front of another's name.
///
/// The list is a thing to maintain, and short on purpose. A key Claude Code
/// invents and this does not learn about is one dialog, once.
pub const PERSON_KEYS: [&str; 4] = [
    "hasCompletedOnboarding",
    "lastOnboardingVersion",
    "tipsHistory",
    "seenNotifications",
];

/// The key holding one entry per directory Claude Code has been run in: the
/// trust it was given there, the tools it was allowed, and that directory's MCP
/// configuration.
const PROJECTS: &str = "projects";

/// Copies what belongs to the person into the Profile a Run is about to launch,
/// or leaves it exactly as it is.
///
/// `into` is the Profile being launched; `default_profile` is where the active
/// Account's state actually lives, which is not that Account's Profile (ADR
/// 0001). Everything else is read from the registry.
///
/// Doing nothing is an ordinary outcome and never a refusal: no Profile to copy
/// from, a client already running against this one, a file that is not the
/// shape it should be. Nor is a write that will not happen — this returns
/// nothing at all, because none of it is worth stopping a Run for. The
/// difference it makes is between a session that starts where the person left
/// off and one that asks them a question first, and a person who answers one
/// question has lost less than one who was refused a client.
pub fn carry(
    host: &dyn Host,
    registry: &Registry,
    email: &str,
    default_profile: &Store,
    into: &Path,
) {
    let destination = probe::identity_file_in(into);
    let Some(mine) = read(host, &destination) else {
        return;
    };

    // The precondition ADR 0003 puts on the write, and the reason this is the
    // Run path's business alone: a client holds this file open and rewrites it
    // wholesale on its way out, so a Profile with one running is a Profile
    // Perch has nothing useful to say about.
    if probe::anything_running(host, into) {
        return;
    }

    let Some(source) = most_recently_used(host, registry, email, default_profile, &destination)
    else {
        return;
    };
    let Some(theirs) = read(host, &source) else {
        return;
    };

    let patched = crossed(host, &theirs, mine.clone());
    if patched == mine {
        return;
    }
    // What crossed was spans of text, taken from a file nothing has parsed.
    // `json::value_at` finds a bare token by running to the `,` or the brace
    // that ends it, and a source truncated mid-token — a client killed on its
    // way out, a disk that filled — has no such byte to stop at, so the span it
    // hands back is `tru` and the splice writes that. The result is a
    // `.claude.json` that is not JSON, and it is the file holding
    // `oauthAccount`: every later read of this Profile's Identity fails on it,
    // while `patch_oauth_account` goes on writing into it because it never
    // parses either.
    //
    // Asked of the result rather than of the source, because the result is what
    // gets written and a source can be ill-formed in ways that splice cleanly.
    // Doing nothing is the ordinary outcome here, so this is a quiet skip like
    // every other reason not to write: nothing carried costs a dialog, and a
    // broken file costs the Account.
    if serde_json::from_str::<serde_json::Value>(&patched).is_err() {
        return;
    }
    // Said rather than swallowed, and said rather than raised. Somebody whose
    // Profile cannot be written will meet the onboarding questions again on
    // every Run, and a remark naming the file is what turns that from a mystery
    // into a `chmod`.
    if let Err(err) = host::write_atomically(host, &destination, &patched) {
        host.note(&format!(
            "{} could not be written ({err}), so this Account starts as though \
             Claude Code had never run for you. The client was launched anyway.",
            destination.display()
        ));
    }
}

/// `mine` with every key that crosses taken from `theirs`, and every other byte
/// of it as it was.
///
/// Each key is a bounded read and a bounded write of its own: the value is
/// found as a span of text and spliced in ([`crate::json`]), so nothing outside
/// the keys named here is reformatted, reordered or read at all. The file is
/// never parsed and never written back wholesale.
fn crossed(host: &dyn Host, theirs: &str, mine: String) -> String {
    let mut patched = mine;
    for key in PERSON_KEYS {
        let Some(value) = json::value_at(theirs, key) else {
            continue;
        };
        if let Some(written) = json::set_value_at(&patched, key, value) {
            patched = written;
        }
    }
    project_entry(host, theirs, patched)
}

/// The same for `projects[<current working directory>]`, which is one entry of
/// one key rather than a key.
///
/// Only the directory the Run was typed in. The rest of `projects` is every
/// other repository the person has ever opened, and an Account does not need
/// the tool approvals of work it is not doing.
fn project_entry(host: &dyn Host, theirs: &str, patched: String) -> String {
    let Ok(here) = host.current_dir() else {
        return patched;
    };
    let here = here.to_string_lossy();

    let Some(entry) =
        json::value_at(theirs, PROJECTS).and_then(|projects| json::value_at(projects, &here))
    else {
        return patched;
    };

    // A Profile that has never been run holds no `projects` at all, which is an
    // empty one for this purpose: the entry is written into it and the key
    // comes into being with it.
    let held = json::value_at(&patched, PROJECTS).unwrap_or("{}");
    let Some(projects) = json::set_value_at(held, &here, entry) else {
        return patched;
    };
    json::set_value_at(&patched, PROJECTS, &projects).unwrap_or(patched)
}

/// The identity file a Run copies from: the most recently used one holding
/// state that may cross into this Profile.
///
/// Which Accounts those are is ADR 0003's scoping, and it is about tool
/// approvals: a Group is a declaration that its Accounts are interchangeable,
/// and carrying a work Account's approvals into a personal Account's session is
/// the one crossing worth preventing. An Account's own earlier state crosses to
/// itself whatever Group it is in, because there is no crossing in it.
///
/// Where an Account's state *is* depends on whether it is the active one: the
/// active Account works in the Default Profile, and its Profile holds only what
/// Perch stored there (ADR 0001). The Profile being launched is never a source
/// — copying a file onto itself does nothing, and a source that is written by
/// every Run would outrank the person's own directory from the first Run
/// onwards.
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

    let mut candidates: Vec<Candidate> = registry
        .accounts
        .iter()
        .filter(|account| may_cross(account, email, group.as_deref()))
        .filter_map(|account| where_it_works(host, registry, account, default_profile))
        .filter(|candidate| candidate.identity_file != destination)
        .collect();

    // Most recently written first, and the directory the person is actually
    // working in ahead of a Profile written at the same instant. A file that
    // will not say when it was written is still a source, and the last one
    // reached for: not knowing is a worse claim than any date.
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
    /// When it was last written, which is when the Account it belongs to was
    /// last used. A file that will not say is a candidate that does not claim
    /// anything.
    used: Option<DateTime<Utc>>,
}

/// Whether an Account's state may cross into the Profile being launched.
fn may_cross(account: &Account, email: &str, group: Option<&str>) -> bool {
    account.email() == email || (group.is_some() && account.group.as_deref() == group)
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

/// A file, where there is one to read.
///
/// A file that is not there, or will not be read, is nothing to copy from and
/// nothing to copy into — both are the ordinary state of a machine rather than
/// a failure, and neither is worth stopping a Run for.
fn read(host: &dyn Host, path: &Path) -> Option<String> {
    host.read_file(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole of the argument for naming what crosses instead of inverting
    /// it: this file holds figures read for one Account, and Perch is the thing
    /// that would show them under another Account's name (ADR 0019).
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

    /// The set is about first-run friction, and each of the three kinds of it
    /// has to be covered or the dialog it prevents comes back.
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
