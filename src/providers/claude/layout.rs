//! Claude's native Default is outside Perch's managed Profiles.

use crate::{Host, Result};

/// The Default Profile, as everything reading or writing the live Credential
/// means it: the directory Claude Code falls back to, and never a Profile.
///
/// `CLAUDE_CONFIG_DIR` is honored, but no directory under Perch's own home is
/// ever the Default Profile — and both a Run and a login point it at one.
pub(super) fn default_profile(host: &dyn Host) -> Result<crate::providers::claude::probe::Store> {
    let told = crate::providers::claude::probe::default_store(host)?;
    let home = crate::holdings::perch_home(host)?;
    if crate::host::is_inside(host, &told.config_dir, &home) {
        return crate::providers::claude::probe::default_profile_store(host);
    }
    Ok(told)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_profile_reached_through_a_link_is_still_not_the_default_profile() {
        let home = "/Users/someone/.config/perch";
        let host = crate::host::FakeHost::new()
            // How somebody comes to have this: a shorter name for the Profiles
            // directory, and a `CLAUDE_CONFIG_DIR` pointing inside it.
            .with_link(
                crate::host::Link::Symbolic,
                format!("{home}/providers/claude/profiles"),
                "/Users/someone/claude",
            )
            .with_env("CLAUDE_CONFIG_DIR", "/Users/someone/claude/work");

        let store = default_profile(&host).expect("a Default Profile is known");

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
            crate::providers::claude::probe::default_profile_store(&host)
                .expect("the real Default Profile")
                .config_dir,
        );
    }
}
