//! Moving everything Perch holds out of `~/.perch` and under `~/.config`.
//!
//! Not a rename. A Profile's keychain namespace is derived from the Profile's
//! path (ADR 0001), so moving the directory would leave every macOS Credential
//! filed under a service name nothing derives any more — every Account
//! Quarantined, with the Credentials still on the machine and unreachable.
//!
//! So each Credential is read from where it is, written where it is going, and
//! only then taken out of where it was. The old home is removed last and only
//! once everything has been read back out of the new one, so a migration that
//! stops half way leaves both copies rather than neither, and running the
//! command again finishes the job.

use std::path::Path;

use crate::credentials;
use crate::error::Result;
use crate::host::Host;
use crate::probe::{self, Store};
use crate::profile;
use crate::registry::{self, Registry};

/// Moves an installation that is still in the old place, if there is one.
///
/// Silent and does nothing at all in the ordinary case, which is every run
/// after the first and every installation that never used the old path.
///
/// Everything it says goes to `host.note`, which is stderr, and not to the
/// command's own writer. The command that triggered this may be a
/// `perch status --json` in somebody's shell prompt: prose on stdout ahead of
/// the document is prose in the middle of whatever is parsing it.
pub fn out_of_the_old_home(host: &dyn Host) -> Result<()> {
    // `$PERCH_HOME` is somebody saying where their state lives. Moving it would
    // be answering a question they have already answered.
    if host.env_var("PERCH_HOME").is_some() {
        return Ok(());
    }

    let old = registry::perch_home_before_the_move(host)?;
    let new = registry::perch_home(host)?;
    if old == new || !host.path_exists(&old.join("registry.json")) {
        return Ok(());
    }
    // Something is already there. Two registries is not a thing to resolve
    // behind somebody's back — the new one wins, and the old one is left where
    // it is for them to look at.
    if host.path_exists(&new.join("registry.json")) {
        host.note(&format!(
            "Perch keeps its state in {} now, and {} is still there from before \
             the move. The one in {} is the one being used; nothing was \
             changed, and the old directory is safe to delete once you have \
             looked at it.",
            new.display(),
            old.display(),
            new.display(),
        ));
        return Ok(());
    }

    // With every other Perch shut out, so two of them starting at once do not
    // each carry half of it across. The lock lives under the new home, which
    // this creates on the way to taking it.
    let mut perch = registry::lock(host)?;
    if host.path_exists(&new.join("registry.json")) {
        return Ok(());
    }

    host.note(&format!(
        "Moving what Perch holds from {} to {}, so it is not sitting in your \
         home directory.",
        old.display(),
        new.display()
    ));
    carry_it_across(host, &mut perch, &old, &new)?;
    host.note("Moved. Nothing else about your Accounts has changed.");
    Ok(())
}

fn carry_it_across(
    host: &dyn Host,
    perch: &mut crate::lock::Held<'_>,
    old: &Path,
    new: &Path,
) -> Result<()> {
    let Some(held) = read_the_old_registry(host, old)? else {
        return Ok(());
    };

    // Every Credential first, because this is the part that can fail: a
    // keychain that will not open stops the migration with the old home
    // untouched, rather than half way through deleting it.
    let mut moved = Vec::new();
    for account in &held.accounts {
        // Through the derivation rather than around it: an address that slugs
        // to nothing resolves to `profiles/` itself, and this loop's last act
        // is to delete the directory it read from. Refused, which leaves both
        // homes where they are and says the registry has to be put right by
        // hand — the migration is the worst possible place to discover it.
        let from = probe::store_for_profile(
            host,
            &registry::profile_dir_in(host, &old.join("profiles"), account.email())?,
        )?;
        let to = probe::store_for_profile(
            host,
            &registry::profile_dir_in(host, &new.join("profiles"), account.email())?,
        )?;

        let Some(credential) = credentials::read(host, &from)? else {
            // An Account with no Credential is already Quarantined, or about to
            // be found so. There is nothing to carry and nothing to lose.
            continue;
        };
        profile::create(host, &to.config_dir, &credential.credential)?;
        carry_the_identity_file(host, &from, &to);
        moved.push(from);
    }

    // The registry last of the writes: until it is there, the new home is not
    // an installation and this runs again from the start.
    registry::save(host, perch, &held)?;

    // And only now what the old home was holding. Best-effort — what matters is
    // already somewhere Claude Code and Perch both read.
    for from in moved {
        for kept_in in credentials::stores_for(host, &from) {
            let _ = kept_in.forget(host);
        }
    }
    let _ = host.remove_dir_all(old);
    Ok(())
}

/// The old registry, read from where it is rather than through
/// [`registry::load`], which reads from wherever Perch keeps state *now* — but
/// read the same way, by the same code.
///
/// The last thing this migration does is delete the old home, so it is the one
/// reader that cannot afford to be lenient: a registry written by a newer Perch
/// has to be refused here exactly as it is refused everywhere else, because
/// parsing it with this build's understanding drops whatever this build does
/// not know about and then removes the only copy that still had it.
fn read_the_old_registry(host: &dyn Host, old: &Path) -> Result<Option<Registry>> {
    registry::read_from(host, &old.join("registry.json"))
}

/// The `.claude.json` a Profile holds, which describes the Account in Claude
/// Code's own terms. Worth carrying and not worth failing over: a Profile
/// without one falls back to the four fields the registry records.
fn carry_the_identity_file(host: &dyn Host, from: &Store, to: &Store) {
    if let Ok(contents) = host.read_file(&from.identity_file) {
        let _ = host.write_file(&to.identity_file, &contents);
    }
}
