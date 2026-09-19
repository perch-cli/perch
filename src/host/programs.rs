//! Executable discovery uses the Host's platform and environment.

use std::path::PathBuf;

use super::Host;

/// Explicit extensions are preserved, including `npm.cmd` for an npm upgrade
/// (ADR an-upgrade-asks-its-channel).
pub fn on_path(host: &dyn Host, name: &str) -> Option<PathBuf> {
    all_on_path(host, name).into_iter().next()
}

/// PATH order is preserved without duplicates so Service installation can
/// rehearse candidates in search order (ADR carried-means-rehearsed).
pub fn all_on_path(host: &dyn Host, name: &str) -> Vec<PathBuf> {
    let Some(path) = host.env_var("PATH") else {
        return Vec::new();
    };

    let on_windows = host.platform() == crate::host::Platform::Windows;
    let separator = if on_windows { ';' } else { ':' };
    // What makes a name executable on Windows is carrying one of PATHEXT's
    // extensions. Lowercase because that is how npm writes `claude.cmd`, and
    // the real filesystem answers case-insensitively anyway.
    let extensions: Vec<String> = if on_windows {
        // The bare name too, and last rather than first, which is the ordering
        // that keeps it safe: npm ships `npm` and `npm.cmd` side by side, and
        // the extensionless one is a shell script Windows cannot run.
        host.env_var("PATHEXT")
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(str::to_lowercase)
            .chain(std::iter::once(String::new()))
            .collect()
    } else {
        vec![String::new()]
    };

    // Rooted directories only, as `curl_at` takes them: an empty element and a
    // `.` both mean the working directory, and `perch upgrade` runs what it
    // finds here.
    let mut found = Vec::new();
    for dir in path.split(separator).filter(|dir| rooted(dir, on_windows)) {
        for extension in &extensions {
            // Joined with '/' rather than `Path::join`, which picks the
            // separator of whatever platform this build runs on: Windows
            // accepts either, and two spellings are two machines.
            let candidate = PathBuf::from(format!("{dir}/{name}{extension}"));
            if host.is_file(&candidate) && !found.contains(&candidate) {
                found.push(candidate);
            }
        }
    }
    found
}

/// Path syntax follows the Host's platform, which can differ from this build's.
pub fn rooted(dir: &str, on_windows: bool) -> bool {
    if dir.starts_with('/') {
        return true;
    }
    // A root of the current drive, and a drive named outright. `C:name` with no
    // separator is relative to that drive's own working directory, which is what
    // is being refused rather than a spelling of the root.
    on_windows
        && (dir.starts_with('\\')
            || matches!(
                dir.as_bytes(),
                [drive, b':', separator, ..]
                    if drive.is_ascii_alphabetic() && matches!(separator, b'\\' | b'/')
            ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{FakeHost, Platform};

    #[test]
    fn windows_finds_a_name_that_already_carries_its_extension() {
        let host = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("PATH", "C:/npm")
            .with_env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .with_file("C:/npm/npm.cmd", "");

        assert_eq!(
            on_path(&host, "npm.cmd"),
            Some(PathBuf::from("C:/npm/npm.cmd"))
        );
    }

    #[test]
    fn windows_prefers_the_spelling_it_can_execute_over_the_bare_name() {
        let host = FakeHost::new()
            .with_platform(Platform::Windows)
            .with_env("PATH", "C:/npm")
            .with_env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
            .with_file("C:/npm/npm", "")
            .with_file("C:/npm/npm.cmd", "");

        assert_eq!(on_path(&host, "npm"), Some(PathBuf::from("C:/npm/npm.cmd")));
    }

    #[test]
    fn every_hit_on_path_is_answered_once_each_in_paths_own_order() {
        let host = FakeHost::new()
            // `/first` twice, as a shell that sources two profiles leaves it.
            .with_env("PATH", "/first:/second:/first")
            .with_file("/first/claude", "")
            .with_file("/second/claude", "");

        assert_eq!(
            all_on_path(&host, "claude"),
            vec![
                PathBuf::from("/first/claude"),
                PathBuf::from("/second/claude")
            ],
        );
    }
}
