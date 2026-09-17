//! Codex's native Default is the home its CLI falls back to, outside Perch's
//! managed Profiles (ADR each-provider-has-a-default).

use std::path::{Path, PathBuf};

use crate::{Host, PerchError, Result};

/// The Default home, as everything reading or writing the live Codex Credential
/// means it: `CODEX_HOME` where it is set, else `~/.codex`, and never a Profile.
/// A Run and a login both point `CODEX_HOME` at a Profile, so a Perch inside one
/// reaches past what it was told.
pub(super) fn default_home(host: &dyn Host) -> Result<PathBuf> {
    let perch_home = crate::holdings::perch_home(host)?;
    if let Some(told) = host.env_var("CODEX_HOME").map(PathBuf::from)
        && !crate::host::is_inside(host, &told, &perch_home)
    {
        return Ok(told);
    }
    host.home_dir()
        .map(|home| home.join(".codex"))
        .map_err(|error| PerchError::Other(error.to_string()))
}

/// The one Credential Store this Switch writes. Codex keeps its login in a
/// file or in the OS keyring by `cli_auth_credentials_store`, and a file Perch
/// writes changes nothing a keyring-backed Codex reads, so a Default that is not
/// file-backed is refused rather than written under. The pin outranks an
/// `auth.json` left behind: a home that moved to the keyring keeps its old file.
pub(super) fn refuse_unless_file_backed(host: &dyn Host, home: &Path) -> Result<()> {
    let file_backed = match store_setting(host, home) {
        Some(store) => store == "file",
        None => {
            host.path_exists(&home.join(super::AUTH_FILE))
                || !host.path_exists(&home.join(CONFIG_FILE))
        }
    };
    if file_backed {
        return Ok(());
    }
    Err(PerchError::Invalid(format!(
        "Codex keeps its login in the keyring, which Perch does not switch. Put \
         `{PIN}` in {} first.",
        home.join(CONFIG_FILE).display()
    )))
}

pub(super) const CONFIG_FILE: &str = "config.toml";
/// The line that makes a Default file-backed, as Codex spells it.
pub(super) const PIN: &str = "cli_auth_credentials_store = \"file\"";

fn store_setting(host: &dyn Host, home: &Path) -> Option<String> {
    store_named(&host.read_file(&home.join(CONFIG_FILE)).ok()?)
}

/// The top-level `cli_auth_credentials_store` a `config.toml` sets, if any.
/// Only the lines before the first table header: the same key under
/// `[profiles.x]` is that profile's, not the Default's.
pub(super) fn store_named(config: &str) -> Option<String> {
    config
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "cli_auth_credentials_store").then(|| {
                value
                    .split('#')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string()
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::FakeHost;

    fn a_home() -> FakeHost {
        FakeHost::new().with_env("HOME", "/Users/someone")
    }

    #[test]
    fn the_default_home_is_codex_home_unless_it_points_inside_perch() {
        let host = a_home().with_env("CODEX_HOME", "/Users/someone/elsewhere");
        assert_eq!(
            default_home(&host).unwrap(),
            PathBuf::from("/Users/someone/elsewhere")
        );
        let host = a_home().with_env(
            "CODEX_HOME",
            "/Users/someone/.config/perch/providers/codex/profiles/x",
        );
        assert_eq!(
            default_home(&host).unwrap(),
            PathBuf::from("/Users/someone/.codex")
        );
    }

    #[test]
    fn a_default_is_file_backed_by_an_auth_file_or_a_pinned_store() {
        let home = Path::new("/Users/someone/.codex");
        assert!(
            refuse_unless_file_backed(&a_home(), home).is_ok(),
            "a home Codex never configured is Perch's to pin"
        );
        let unpinned = a_home().with_file(home.join("config.toml"), "model = \"gpt-5\"\n");
        assert!(refuse_unless_file_backed(&unpinned, home).is_err());
        let pinned = a_home().with_file(
            home.join("config.toml"),
            "model = \"gpt-5\"\ncli_auth_credentials_store = \"file\" # kept\n",
        );
        assert!(refuse_unless_file_backed(&pinned, home).is_ok());
        let keyring = a_home().with_file(
            home.join("config.toml"),
            "cli_auth_credentials_store = \"keyring\"\n",
        );
        assert!(refuse_unless_file_backed(&keyring, home).is_err());
        let logged_in = a_home().with_file(home.join("auth.json"), "{}");
        assert!(refuse_unless_file_backed(&logged_in, home).is_ok());
        let moved_to_the_keyring = a_home().with_file(home.join("auth.json"), "{}").with_file(
            home.join("config.toml"),
            "cli_auth_credentials_store = \"keyring\"\n",
        );
        assert!(
            refuse_unless_file_backed(&moved_to_the_keyring, home).is_err(),
            "the pin outranks a file left behind"
        );
        let in_a_profile = a_home().with_file(
            home.join("config.toml"),
            "model = \"gpt-5\"\n[profiles.work]\ncli_auth_credentials_store = \"file\"\n",
        );
        assert!(
            refuse_unless_file_backed(&in_a_profile, home).is_err(),
            "a profile's pin is not the Default's"
        );
    }
}
