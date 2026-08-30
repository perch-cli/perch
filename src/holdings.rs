//! Where the Holdings sit on this machine: every path under `$PERCH_HOME`, and
//! the two locks taken inside it.
//!
//! Derived rather than recorded. The registry names Accounts and Groups and
//! says nowhere what any of them is kept in, so a Credential Store follows its
//! Profile's path and moving `profiles/` is not a rename.
//!
//! Below [`crate::registry`] rather than inside it: a path is asked for by
//! modules that read no document, and this is the lowest one reaching
//! everything a path names (ADR code-lives-where-it-reaches).

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{PerchError, Result};
use crate::host::Host;
use crate::lock::{self, LockSpec};

/// `$PERCH_HOME`, or `~/.config/perch` — an error when neither is knowable,
/// rather than a registry written into the filesystem root.
///
/// The same path on every platform rather than `%APPDATA%`, because one rule is
/// easier to keep in a Host port exposing only a home directory. A preference.
pub fn perch_home(host: &dyn Host) -> Result<PathBuf> {
    // Set-but-empty is the machine not saying: taken at face value it makes the
    // registry a relative path, so Perch would read and write the Holdings
    // wherever it happened to be invoked from.
    if let Some(overridden) = host
        .env_var("PERCH_HOME")
        .filter(|overridden| !overridden.is_empty())
    {
        // A relative one is the same harm the empty case is refused for, and it
        // is the one a Service carries into a unit file to resolve against a
        // working directory nobody chose.
        if !names_one_place(host.platform(), &overridden) {
            return Err(PerchError::Invalid(format!(
                "PERCH_HOME is set to `{}`, which is not an absolute path, so \
                 where Perch holds the Holdings would depend on the directory \
                 each command was run from.\n\
                 Set it to a full path, or unset it for {}.",
                overridden,
                home_dir(host)
                    .map(|home| home.join(".config").join("perch").display().to_string())
                    .unwrap_or_else(|_| "~/.config/perch".to_string()),
            )));
        }
        return Ok(PathBuf::from(overridden));
    }
    Ok(home_dir(host)?.join(".config").join("perch"))
}

/// Whether a path names the same place from every working directory.
///
/// Asked of the Host's platform rather than of `Path::is_absolute`, which is
/// compiled for the machine running the code: a fake standing in for Windows
/// would otherwise answer as the Linux it runs on.
fn names_one_place(platform: crate::host::Platform, text: &str) -> bool {
    match platform {
        crate::host::Platform::Windows => {
            let mut bytes = text.bytes();
            // A UNC path, or a drive letter and a separator. A leading `\\` alone
            // is relative to whichever drive the process is on.
            text.starts_with(r"\\")
                || matches!(
                    (bytes.next(), bytes.next(), bytes.next()),
                    (Some(letter), Some(b':'), Some(b'\\' | b'/')) if letter.is_ascii_alphabetic()
                )
        }
        crate::host::Platform::MacOs | crate::host::Platform::Other => text.starts_with('/'),
    }
}

fn home_dir(host: &dyn Host) -> Result<PathBuf> {
    host.home_dir()
        .map_err(|err| PerchError::Other(err.to_string()))
}

pub fn registry_path(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("registry.json"))
}

pub fn profiles_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("profiles"))
}

/// The Default Profile, as everything reading or writing the live Credential
/// means it: the directory Claude Code falls back to, and never a Profile.
///
/// `CLAUDE_CONFIG_DIR` is honored, but no directory under Perch's own home is
/// ever the Default Profile — and both a Run and a login point it at one.
pub fn the_default_profile(host: &dyn Host) -> Result<crate::probe::Store> {
    let told = crate::probe::default_store(host)?;
    let home = perch_home(host)?;
    if crate::host::is_inside(host, &told.config_dir, &home) {
        return crate::probe::default_profile_store(host);
    }
    Ok(told)
}

/// The Profile directory for an Account. The email is slugged because the path is
/// hashed into a keychain service name and has to be stable and printable.
///
/// An address that slugs to nothing is refused here, at the one place every store
/// is derived from.
pub fn profile_dir_for(host: &dyn Host, email: &str) -> Result<PathBuf> {
    let profiles = profiles_dir(host)?;
    let slugged = slug(email);
    let dir = profiles.join(&slugged);

    // Two ways of asking one question, because the answer is the whole machine:
    // an empty slug, and a path that is not one directory below `profiles/`.
    if slugged.is_empty() || dir.parent() != Some(profiles.as_path()) {
        return Err(PerchError::Invalid(format!(
            "`{email}` has no character a Profile directory can be named after, \
             so Perch cannot say where its Credential would be kept.\n\
             An Account recorded under that address has to be removed from \
             {} by hand.",
            registry_path(host)?.display(),
        )));
    }
    Ok(dir)
}

/// Where a login lives while Perch is running it.
///
/// Named after the moment it started, because a Profile is named after the
/// Account it holds and which Account that is only becomes knowable once the
/// login has finished (ADR a-login-perch-does-not-need).
pub fn pending_login_dir(host: &dyn Host, started_at: DateTime<Utc>) -> Result<PathBuf> {
    Ok(pending_logins_dir(host)?.join(format!("login-{}", started_at.timestamp_millis())))
}

/// Where every pending login lives, so the ones nobody came back from can be
/// found again.
pub fn pending_logins_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("pending"))
}

/// When the login that made this directory started, as its name records.
/// `login-<millis>`, written by [`pending_login_dir`], and the only account of
/// the directory's age that nothing later moves.
pub fn pending_login_started_at(dir: &Path) -> Option<DateTime<Utc>> {
    let millis: i64 = dir
        .file_name()?
        .to_str()?
        .strip_prefix("login-")?
        .parse()
        .ok()?;
    DateTime::from_timestamp_millis(millis)
}

/// Where every Triage writes what it hands over.
///
/// Under `$PERCH_HOME` so a Purge takes it back, and carrying no version of its
/// own: what a Triage leaves is evidence rather than one of the Holdings, so
/// nothing migrates it and no Export carries it (ADR a-trail-is-evidence).
pub fn triage_dir(host: &dyn Host) -> Result<PathBuf> {
    Ok(perch_home(host)?.join("triage"))
}

/// Where one Triage writes, named after the moment it started — as a pending
/// login is, and for its reason: the name is the only account of the
/// directory's age that nothing later moves. Milliseconds are fixed width for
/// the next two centuries, so the runs sort in the order they were written and
/// pruning the oldest needs no second reading.
pub fn triage_run_dir(host: &dyn Host, started_at: DateTime<Utc>) -> Result<PathBuf> {
    Ok(triage_dir(host)?.join(format!("run-{}", started_at.timestamp_millis())))
}

/// When the Triage that made this directory started, as its name records.
/// `run-<millis>`, written by [`triage_run_dir`], and what tells one of its runs
/// from anything else somebody left in there.
pub fn triage_run_started_at(dir: &Path) -> Option<DateTime<Utc>> {
    let millis: i64 = dir
        .file_name()?
        .to_str()?
        .strip_prefix("run-")?
        .parse()
        .ok()?;
    DateTime::from_timestamp_millis(millis)
}

/// Whether two Accounts derive the same Profile directory. The derivation is
/// `profiles_dir` joined with the slugged email, so sharing a slug is sharing
/// a Profile — kept here beside the derivation so the two cannot drift apart.
pub fn same_profile(one: &str, other: &str) -> bool {
    slug(one) == slug(other)
}

pub fn slug(email: &str) -> String {
    let mut slugged = String::with_capacity(email.len());
    slug_into(&mut slugged, email);
    slugged
}

/// The same, into a buffer the caller keeps, so a scan comparing slugs
/// allocates once rather than once per candidate. Lowercased character by
/// character rather than through `str::to_lowercase`, which would allocate a
/// second string: the one mapping the two disagree on is Greek's final sigma,
/// and `ς` and `σ` are both written `-` here.
pub(crate) fn slug_into(slugged: &mut String, email: &str) {
    slugged.clear();
    slugged.extend(
        email
            .chars()
            .flat_map(char::to_lowercase)
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }),
    );
    let end = slugged.trim_end_matches('-').len();
    slugged.truncate(end);
    let start = slugged.len() - slugged.trim_start_matches('-').len();
    slugged.drain(..start);
}

/// How long a Perch that died holding the registry lock keeps it.
///
/// Longer than the Claude Code locks a Switch takes, because it is the outer
/// lock; short enough that a killed Perch leaves a usable machine within a
/// minute.
pub(crate) const REGISTRY_STALE_MILLIS: i64 = 90_000;

const REGISTRY_UPDATE_MILLIS: i64 = 5_000;

/// The lock one Perch takes so that no other Perch is changing the registry at
/// the same time.
///
/// A directory, taken with the same `mkdir`-or-fail primitive the Claude Code
/// locks use: the call both asks and answers, with nothing in between.
pub fn lock_spec(host: &dyn Host) -> Result<LockSpec> {
    Ok(LockSpec {
        name: "the Perch registry lock",
        held_by: "the other `perch`",
        dir: perch_home(host)?.join(".registry.lock"),
        stale_millis: REGISTRY_STALE_MILLIS,
        update_millis: REGISTRY_UPDATE_MILLIS,
        lost_means: "Another `perch` has been changing the registry since this \
                     command read it, so what this one holds in memory is behind \
                     what is on disk. Nothing of it will be written over theirs.",
    })
}

/// How long a Watcher that died holding the watcher lock keeps it.
///
/// Derived rather than chosen: the longest a healthy Watcher goes quiet is its
/// longest wait between rounds plus the round after it. The number is here and
/// the derivation is asserted in `watch`, where both its terms live.
pub(crate) const WATCHER_STALE_MILLIS: i64 = 1_350_000;

/// Comfortably inside a round, so the renewal a round makes always touches.
const WATCHER_UPDATE_MILLIS: i64 = 60_000;

/// The lock that makes a Watcher the only one on this machine
/// (ADR the-machine-runs-the-watcher).
///
/// Two loops each keep their Cooldown in memory, where neither can see the
/// other's, so running the thing twice undoes the pacing. A Check too.
pub fn watcher_lock_spec(host: &dyn Host) -> Result<LockSpec> {
    Ok(LockSpec {
        name: "the Perch watcher lock",
        // The Watcher rather than one of its three arrangements: which of them
        // holds this neither changes what to do about it nor is knowable here.
        held_by: "another Watcher",
        dir: perch_home(host)?.join(".watch.lock"),
        stale_millis: WATCHER_STALE_MILLIS,
        update_millis: WATCHER_UPDATE_MILLIS,
        lost_means: "Another Watcher has taken over watching this machine, so \
                     this one is no longer the only one deciding. It stops \
                     rather than deciding alongside it.",
    })
}

/// Shuts every other Perch out of the registry until the hold is dropped.
///
/// The hold spans the command rather than the write, because it is the *read*
/// that goes stale: a copy saved after somebody else's Switch landed would put
/// `active` back and send the next Capture to the wrong Profile.
pub fn lock(host: &dyn Host) -> Result<lock::Held<'_>> {
    lock::take_all(host, vec![lock_spec(host)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_email_slugs_to_a_stable_directory_name() {
        assert_eq!(slug("Someone@Example.com"), "someone-example-com");
    }

    /// The mapping `str::to_lowercase` and `char::to_lowercase` disagree on, and
    /// the reason a slug may be taken character by character. A registry v0.2.0
    /// wrote came back refused over the same rule read the other way.
    #[test]
    fn an_address_ending_in_a_greek_sigma_slugs_the_same_whichever_case_it_carries() {
        assert_eq!(
            slug("XPONO\u{3a3}@example.com"),
            slug("xpono\u{3c2}@example.com")
        );
        assert_eq!(
            slug("XPONO\u{3a3}@example.com"),
            slug("xpono\u{3c3}@example.com")
        );
        assert_eq!(slug("\u{3a3}\u{3a3}"), "");
    }

    /// A slug is trimmed at both ends, and the trim is what a buffer reused
    /// across a scan has to leave behind along with the rest of the last one.
    #[test]
    fn a_scan_comparing_slugs_leaves_nothing_of_the_last_one_in_its_buffer() {
        let mut buffer = String::new();
        slug_into(&mut buffer, "a-much-longer-address@example.com");
        slug_into(&mut buffer, "@ab@");

        assert_eq!(buffer, "ab");
    }

    #[test]
    fn an_address_that_names_no_directory_is_refused_rather_than_naming_them_all() {
        let host = crate::host::FakeHost::new();
        let profiles = profiles_dir(&host).unwrap();

        for degenerate in ["@", "-", "...", "@.-@"] {
            assert_eq!(slug(degenerate), "", "the case this is about: {degenerate}");
            let refused =
                profile_dir_for(&host, degenerate).expect_err("no Profile can be named after this");
            assert!(refused.to_string().contains(degenerate), "{refused}");
        }

        assert_eq!(
            profile_dir_for(&host, "someone@example.com").unwrap(),
            profiles.join("someone-example-com"),
            "and an ordinary address is unaffected"
        );
    }

    #[test]
    fn one_perch_changes_the_registry_at_a_time() {
        let host = crate::host::FakeHost::new();

        let held = lock(&host).expect("the first Perch takes it");
        let refused = match lock(&host) {
            Err(refused) => refused,
            Ok(_) => panic!("the second Perch must wait, then give up"),
        };
        assert!(
            refused.to_string().contains("the Perch registry lock"),
            "{refused}"
        );

        drop(held);
        lock(&host).expect("a lock given back can be taken again");
    }

    /// On a fresh machine the *lock* is what brings Perch's home into being,
    /// before any registry has been written into it.
    #[test]
    fn the_home_the_lock_creates_is_the_owners_alone() {
        let host = crate::host::FakeHost::new();

        let _perch = lock(&host).expect("the registry lock is free");

        assert_eq!(
            host.mode_of(perch_home(&host).unwrap()),
            Some(crate::host::PRIVATE_DIR_MODE)
        );
    }

    #[test]
    fn a_profile_reached_through_a_link_is_still_not_the_default_profile() {
        let home = "/Users/someone/.config/perch";
        let host = crate::host::FakeHost::new()
            // How somebody comes to have this: a shorter name for the Profiles
            // directory, and a `CLAUDE_CONFIG_DIR` pointing inside it.
            .with_link(
                crate::host::Link::Symbolic,
                format!("{home}/profiles"),
                "/Users/someone/claude",
            )
            .with_env("CLAUDE_CONFIG_DIR", "/Users/someone/claude/work");

        let store = the_default_profile(&host).expect("a Default Profile is known");

        assert!(
            !crate::host::is_inside(
                &host,
                &store.config_dir,
                std::path::Path::new("/Users/someone/claude")
            ),
            "a Profile is never the Default Profile, whichever name reaches it: {:?}",
            store.config_dir
        );
        assert_eq!(
            store.config_dir,
            crate::probe::default_profile_store(&host)
                .expect("the real Default Profile")
                .config_dir,
        );
    }

    #[test]
    fn perch_home_is_taken_from_the_environment_verbatim_when_it_is_set() {
        let host = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("PERCH_HOME", "/tmp/somewhere-else");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/tmp/somewhere-else")
        );
        assert_eq!(
            registry_path(&host).unwrap(),
            std::path::PathBuf::from("/tmp/somewhere-else/registry.json"),
            "and everything under it moves with it"
        );
    }

    #[test]
    fn without_the_override_perch_keeps_its_registry_under_the_config_directory() {
        let host = crate::host::FakeHost::new().with_env("HOME", "/Users/someone");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/Users/someone/.config/perch")
        );
    }

    /// The same harm as the empty case, arrived at by naming somewhere rather
    /// than nowhere: `PERCH_HOME=perch-data` makes every directory its own
    /// machine, each looking empty to the next and adopting again.
    #[test]
    fn a_perch_home_that_names_no_one_place_is_refused_rather_than_followed() {
        for (platform, relative, absolute) in [
            (crate::host::Platform::Other, "perch-data", "/srv/perch"),
            (crate::host::Platform::MacOs, "../perch", "/srv/perch"),
            (
                crate::host::Platform::Windows,
                r"work\perch",
                r"C:\work\perch",
            ),
            (
                crate::host::Platform::Windows,
                r"\perch",
                r"\\host\share\perch",
            ),
        ] {
            let host = crate::host::FakeHost::new()
                .with_platform(platform)
                .with_env("HOME", "/Users/someone")
                .with_env("PERCH_HOME", relative);

            let refused = perch_home(&host).expect_err("{relative} names no one place");
            assert_eq!(refused.exit_code(), crate::error::EXIT_INVALID);
            assert!(
                refused.to_string().contains(relative),
                "the refusal names the value: {refused}"
            );

            let host = crate::host::FakeHost::new()
                .with_platform(platform)
                .with_env("HOME", "/Users/someone")
                .with_env("PERCH_HOME", absolute);
            assert_eq!(
                perch_home(&host).unwrap(),
                std::path::PathBuf::from(absolute),
                "and one that does is taken as it stands, on {platform:?}"
            );
        }
    }

    /// `export PERCH_HOME=$SOMETHING_UNSET` is the ordinary way to arrive here,
    /// and a relative registry path is the Holdings following the working
    /// directory around.
    #[test]
    fn a_perch_home_set_to_nothing_is_the_machine_not_saying_rather_than_the_working_directory() {
        let host = crate::host::FakeHost::new()
            .with_env("HOME", "/Users/someone")
            .with_env("PERCH_HOME", "");

        assert_eq!(
            perch_home(&host).unwrap(),
            std::path::PathBuf::from("/Users/someone/.config/perch")
        );
    }
}
