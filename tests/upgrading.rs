//! Behavior: what `perch upgrade` does about this machine's Installation.
//!
//! The whole of the command is deciding which Channel made the Installation and
//! handing the work back to it (ADR an-upgrade-asks-its-channel), so most of
//! what is asserted here is *which program was run* — and, for the two Channels
//! Perch does not run anything for, that nothing was.
//!
//! Nothing here arranges a registry, and one test asserts that on purpose.

mod common;

use common::*;
use perch::commands::upgrade::{self, UpgradeArgs};
use perch::error::{EXIT_NOTHING_TO_DO, EXIT_OK, PerchError};
use perch::host::fake::Effect;
use perch::host::{FakeHost, Platform};
use perch::upgrade::LATEST_URL;

/// A program that was run with the terminal attached: what it was, what it was
/// given, and what was added to its environment.
type Launched = (String, Vec<String>, Vec<(String, String)>);

/// Unambiguously newer and older than whatever this build happens to be, so
/// these tests survive the version being bumped by a Release.
const NEWER: &str = "999.0.0";
const OLDER: &str = "0.0.1";

fn latest(version: &str) -> String {
    format!(r#"{{"tag_name":"v{version}","name":"whatever"}}"#)
}

/// A machine where the newest published Release is newer than this build.
fn machine() -> FakeHost {
    machine_with_claude_code().with_reply(LATEST_URL, 200, &latest(NEWER))
}

/// The `brew` beside a Cellar, which every Homebrew Installation has and which
/// `homebrew_command` asks for before it hands the work over.
fn with_brew(host: FakeHost) -> FakeHost {
    host.with_file("/opt/homebrew/bin/brew", "")
}

fn upgrading(host: &FakeHost, args: UpgradeArgs) -> (perch::Result<i32>, String) {
    let mut out = Vec::new();
    let outcome = upgrade::run(host, args, &mut out);
    (outcome, String::from_utf8(out).expect("it said text"))
}

fn ran(host: &FakeHost) -> Vec<Launched> {
    host.effects()
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::ExecInteractive {
                program, args, env, ..
            } => Some((program, args, env)),
            _ => None,
        })
        .collect()
}

// ---- the Channels Perch hands the work back to ---------------------------

#[test]
fn a_homebrew_installation_is_handed_to_the_brew_that_owns_it() {
    let host = with_brew(machine()).installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, said) = upgrading(&host, UpgradeArgs::default());

    assert_eq!(outcome.expect("it ran brew"), EXIT_OK);
    assert_eq!(
        ran(&host)
            .into_iter()
            .map(|(program, args, _)| (program, args))
            .collect::<Vec<_>>(),
        vec![(
            "/opt/homebrew/bin/brew".to_string(),
            vec!["upgrade".to_string(), "perch".to_string()]
        )]
    );
    assert!(
        said.contains("/opt/homebrew/bin/brew upgrade perch"),
        "the command is said before it is run, because handing the terminal to \
         `brew` for two minutes without saying so reads as a hang: {said}"
    );
}

/// Homebrew refuses `--release`, so the Release Perch resolves decides nothing
/// but whether it says "already the newest" — and unauthenticated
/// `api.github.com` allows 60 requests an hour per address. An Upgrade that
/// stopped there is one `brew upgrade perch` would have made.
#[test]
fn a_homebrew_upgrade_is_handed_over_even_where_github_will_not_answer() {
    // 403 is what a shared address gets from the unauthenticated API.
    let host = with_brew(machine_with_claude_code().with_reply(LATEST_URL, 403, "{}"))
        .installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, said) = upgrading(&host, UpgradeArgs::default());

    assert_eq!(outcome.expect("it ran brew"), EXIT_OK, "{said}");
    assert_eq!(
        ran(&host)
            .into_iter()
            .map(|(program, args, _)| (program, args))
            .next(),
        Some((
            "/opt/homebrew/bin/brew".to_string(),
            vec!["upgrade".to_string(), "perch".to_string()]
        )),
        "{said}"
    );
    assert!(
        host.notes().iter().any(|note| note.contains("Homebrew")),
        "and it says the compare was lost rather than doing it quietly: {:?}",
        host.notes()
    );
}

#[test]
fn an_npm_installation_is_handed_to_npm() {
    let host = machine().with_file("/usr/bin/npm", "").installed_at(
        "/usr/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
    );

    let (outcome, said) = upgrading(&host, UpgradeArgs::default());

    assert_eq!(outcome.expect("it ran npm"), EXIT_OK);
    assert_eq!(
        ran(&host)
            .into_iter()
            .map(|(program, args, _)| (program, args))
            .collect::<Vec<_>>(),
        vec![(
            "/usr/bin/npm".to_string(),
            vec![
                "update".to_string(),
                "-g".to_string(),
                "perch-cli".to_string()
            ]
        )]
    );
    assert!(said.contains("npm update -g perch-cli"), "{said}");
}

/// The refusal named the command to type once `brew` is back, and could only
/// ever fire for `--channel homebrew`: a detected prefix was taken to have a
/// `bin/brew` under it, so a Homebrew moved or half removed surfaced as
/// "could not run /opt/homebrew/bin/brew upgrade perch: No such file".
#[test]
fn a_cellar_whose_brew_has_gone_says_the_command_rather_than_failing_to_run_it() {
    let host = machine().installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, _said) = upgrading(&host, UpgradeArgs::default());

    let refused = outcome.expect_err("there is no brew to hand it back to");
    assert!(ran(&host).is_empty(), "nothing was run: {:?}", ran(&host));
    assert!(
        refused.to_string().contains("brew upgrade perch"),
        "it says the command to type once `brew` is back: {refused}"
    );
}

/// A `0` here is a script's `perch upgrade && restart-my-thing` restarting the
/// old binary on the strength of an Upgrade that only printed a suggestion.
#[test]
fn npm_on_windows_is_printed_rather_than_run_because_it_cannot_work_from_here() {
    let host = machine()
        .with_platform(Platform::Windows)
        .with_env("PATHEXT", ".COM;.EXE;.BAT;.CMD")
        .with_file("/usr/bin/npm.cmd", "")
        .installed_at("/c/Users/someone/AppData/Roaming/npm/node_modules/perch-cli/node_modules/@perch-cli/win32-x64/bin/perch.exe");

    let (outcome, _said) = upgrading(&host, UpgradeArgs::default());

    let said = outcome
        .expect_err("nothing was upgraded, so it does not report one")
        .to_string();
    assert_eq!(
        PerchError::NothingToDo(String::new()).exit_code(),
        EXIT_NOTHING_TO_DO
    );
    assert!(ran(&host).is_empty(), "nothing was run: {:?}", ran(&host));
    assert!(
        said.contains("update -g perch-cli") && said.contains("running"),
        "it says the command and why Perch is not the one to run it: {said}"
    );
    assert!(
        said.contains("Nothing was upgraded"),
        "and says plainly that the machine is as it was: {said}"
    );
}

// ---- the one Channel Perch replaces itself for ---------------------------

#[test]
fn an_installer_installation_runs_the_embedded_installer_at_the_tag() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    let (outcome, said) = upgrading(&host, UpgradeArgs::default());

    assert_eq!(outcome.expect("it ran the installer"), EXIT_OK);
    let launched = ran(&host);
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].0, "/bin/sh");
    // As a path rather than as a string: `perch_home` is spelled with whatever
    // separator the build prefers, and on Windows `/` and `\` are one.
    assert_eq!(
        std::path::Path::new(&launched[0].1[0]),
        std::path::Path::new("/Users/someone/.config/perch/perch-upgrade.sh")
    );
    assert_eq!(
        launched[0].2,
        vec![("PERCH_VERSION".to_string(), format!("v{NEWER}"))],
        "the tag reaches the script, because one that does not is an upgrade to \
         whatever happened to be newest"
    );
    assert!(said.contains(&format!("v{NEWER}")), "{said}");
}

/// The Windows installer's default is `%LOCALAPPDATA%\Perch\bin`, which is not
/// the Unix one.
#[test]
fn a_windows_installer_installation_is_recognized_where_windows_puts_it() {
    let host =
        windows_machine().installed_at("C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe");

    let (outcome, _) = upgrading(&host, UpgradeArgs::default());

    outcome.expect("it ran the installer");
    let launched = ran(&host);
    assert_eq!(launched.len(), 1);
    assert_eq!(
        launched[0].0,
        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
    );
    assert_eq!(
        launched[0].2,
        vec![("PERCH_VERSION".to_string(), format!("v{NEWER}"))]
    );
}

fn windows_machine() -> perch::host::FakeHost {
    machine()
        .with_platform(Platform::Windows)
        .with_env("LOCALAPPDATA", "C:\\Users\\someone\\AppData\\Local")
        .with_env("SystemRoot", "C:\\Windows")
}

/// A bare name is searched for in the working directory before `PATH`, so a
/// downloads folder holding a `powershell.exe` would answer instead — with
/// `-ExecutionPolicy Bypass`, and a script that replaces the Perch binary
/// (ADR a-crate-must-not-cost-a-seam).
#[test]
fn the_installer_is_run_by_the_powershell_windows_says_it_has() {
    let host =
        windows_machine().installed_at("C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe");

    upgrading(&host, UpgradeArgs::default())
        .0
        .expect("it ran the installer");

    assert_eq!(
        ran(&host)[0].0,
        "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
    );
}

/// The same answer `curl` gets on a machine that will not say where Windows is.
#[test]
fn a_windows_that_does_not_say_where_it_is_installed_runs_no_installer() {
    let host = machine()
        .with_platform(Platform::Windows)
        .with_env("LOCALAPPDATA", "C:\\Users\\someone\\AppData\\Local")
        .installed_at("C:\\Users\\someone\\AppData\\Local\\Perch\\bin\\perch.exe");

    let (outcome, _) = upgrading(&host, UpgradeArgs::default());

    let said = outcome.expect_err("it refused").to_string();
    assert!(said.contains("SystemRoot"), "{said}");
    assert!(ran(&host).is_empty(), "nothing was run");
}

#[test]
fn an_installer_installation_somewhere_chosen_is_still_the_installers() {
    let host = machine()
        .with_env("PERCH_INSTALL_DIR", "/opt/mine")
        .installed_at("/opt/mine/perch");

    let (outcome, _) = upgrading(&host, UpgradeArgs::default());

    outcome.expect("it ran the installer");
    assert_eq!(ran(&host).len(), 1);
}

#[test]
fn the_embedded_installer_is_cleared_away_afterwards() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    upgrading(&host, UpgradeArgs::default())
        .0
        .expect("it ran the installer");

    assert_eq!(
        host.file("/Users/someone/.config/perch/perch-upgrade.sh"),
        None
    );
}

/// Asserted on the effect rather than on the file, which is deliberately not
/// there afterwards.
#[test]
fn the_embedded_installer_is_written_for_its_owner_alone() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    upgrading(&host, UpgradeArgs::default())
        .0
        .expect("it ran the installer");

    let script = std::path::Path::new("/Users/someone/.config/perch/perch-upgrade.sh");
    assert!(
        host.effects().iter().any(|effect| matches!(
            effect,
            Effect::WrotePrivateFile(path) if path == script
        )),
        "{:?}",
        host.effects()
    );
}

#[test]
fn the_installer_that_runs_is_the_one_embedded_in_the_binary() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");
    let mut out = Vec::new();

    // Read back before the command clears it, by asking the fake what the
    // installer would have been handed.
    let (name, embedded) = perch::upgrade::installer_for(Platform::MacOs);
    assert_eq!(name, "perch-upgrade.sh");
    assert!(
        embedded.contains("PERCH_VERSION") && embedded.contains("SHA256SUMS"),
        "the embedded copy is the installer, checks and all"
    );

    upgrade::run(&host, UpgradeArgs::default(), &mut out).expect("it ran the installer");
}

// ---- the Installation Perch will not touch -------------------------------

#[test]
fn a_binary_perch_did_not_place_is_refused_rather_than_written_over() {
    for exe in ["/usr/local/bin/perch", "/opt/perch/perch"] {
        let host = machine().installed_at(exe);

        let (outcome, _) = upgrading(&host, UpgradeArgs::default());

        let refused = outcome.expect_err("it will not write over a stranger");
        assert!(matches!(refused, PerchError::Invalid(_)), "{refused}");
        assert!(
            refused.to_string().contains(exe),
            "it names the path it found, so the person can see what Perch saw: \
             {refused}"
        );
        assert!(refused.to_string().contains("--channel"), "{refused}");
        assert!(ran(&host).is_empty(), "and nothing was run");
    }
}

/// A check writes nothing, so the refusal that protects a hand-placed binary
/// has nothing to protect here. The Channel is the one thing it cannot answer,
/// so that is `null` and the two facts a script came for are still there.
#[test]
fn a_check_answers_on_a_binary_perch_did_not_place_rather_than_refusing() {
    let host = machine().installed_at("/usr/local/bin/perch");

    let (outcome, said) = upgrading(
        &host,
        UpgradeArgs {
            check: true,
            json: true,
            ..UpgradeArgs::default()
        },
    );

    let code = outcome.expect("a question is answered, not refused");
    assert_eq!(code, EXIT_OK, "{said}");
    let answered: serde_json::Value =
        serde_json::from_str(&said).unwrap_or_else(|_| panic!("it is a document: {said}"));
    assert_eq!(
        answered["installed"],
        upgrade_installed(),
        "the facts a script came for are there: {said}"
    );
    assert!(answered["newest"].is_string(), "{said}");
    assert_eq!(
        answered["channel"],
        serde_json::Value::Null,
        "and the one thing that cannot be answered says so: {said}"
    );
    assert!(ran(&host).is_empty(), "a check runs nothing");
}

#[test]
fn a_named_channel_is_taken_over_what_the_path_says() {
    let host = machine().installed_at("/opt/perch/perch");

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            channel: Some("installer".to_string()),
            ..UpgradeArgs::default()
        },
    );

    assert_eq!(outcome.expect("it believed the flag"), EXIT_OK);
    assert_eq!(ran(&host).len(), 1);
}

/// `brew` is taken as well as `homebrew`, which is what people type.
#[test]
fn a_named_homebrew_finds_its_brew_on_the_path() {
    for word in ["homebrew", "brew", "Homebrew"] {
        let host = machine()
            .with_file("/usr/bin/brew", "")
            .installed_at("/opt/perch/perch");

        let (outcome, _) = upgrading(
            &host,
            UpgradeArgs {
                channel: Some(word.to_string()),
                ..UpgradeArgs::default()
            },
        );

        outcome.unwrap_or_else(|err| panic!("`--channel {word}`: {err}"));
        assert_eq!(ran(&host)[0].0, "/usr/bin/brew", "{word}");
    }
}

#[test]
fn a_channel_whose_own_tool_is_missing_says_which_command_would_have_run() {
    // A *named* Homebrew carries no prefix, so nothing but `PATH` can answer
    // and nothing is on it. A detected one carries the prefix its Cellar sits
    // under, which is an answer by construction and cannot reach this.
    let named = machine().installed_at("/opt/perch/perch");
    let (outcome, _) = upgrading(
        &named,
        UpgradeArgs {
            channel: Some("homebrew".to_string()),
            ..UpgradeArgs::default()
        },
    );
    let refused = outcome.expect_err("there is no brew");
    assert!(
        refused.to_string().contains("brew upgrade perch"),
        "{refused}"
    );
    assert!(ran(&named).is_empty());

    let npmless = machine().installed_at(
        "/usr/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
    );
    let (outcome, _) = upgrading(&npmless, UpgradeArgs::default());
    let refused = outcome.expect_err("there is no npm");
    assert!(
        refused.to_string().contains("npm update -g perch-cli"),
        "{refused}"
    );
    assert!(ran(&npmless).is_empty());
}

#[test]
fn a_check_names_the_channel_in_the_words_the_flag_takes() {
    for (exe, word) in [
        ("/opt/homebrew/Cellar/perch/0.1.1/bin/perch", "homebrew"),
        (
            "/usr/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
            "npm",
        ),
        ("/Users/someone/.local/bin/perch", "installer"),
    ] {
        let host = machine().installed_at(exe);
        let (outcome, said) = upgrading(
            &host,
            UpgradeArgs {
                check: true,
                json: true,
                ..UpgradeArgs::default()
            },
        );

        outcome.expect("a check succeeded");
        let document: serde_json::Value = serde_json::from_str(&said).expect("a document");
        assert_eq!(document["channel"], word, "{exe}");
    }
}

#[test]
fn a_channel_that_is_not_one_is_refused_by_name() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            channel: Some("apt".to_string()),
            ..UpgradeArgs::default()
        },
    );

    let refused = outcome.expect_err("apt is not a Channel");
    assert!(refused.to_string().contains("homebrew"), "{refused}");
    assert!(refused.to_string().contains("apt"), "{refused}");
}

/// A check may go without the answer read off the *path* — that is what lets a
/// hand-unpacked Perch ask what is newest — and a word somebody typed wrongly
/// is not that answer.
#[test]
fn a_check_refuses_a_channel_word_that_is_not_one_rather_than_reporting_none() {
    let host = with_brew(machine()).installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, said) = upgrading(
        &host,
        UpgradeArgs {
            check: true,
            channel: Some("homebre".to_string()),
            ..UpgradeArgs::default()
        },
    );

    let refused = outcome.expect_err("`homebre` is a typo rather than a Channel");
    assert!(refused.to_string().contains("homebre"), "{refused}");
    assert!(refused.to_string().contains("`npm`"), "{refused}");
    assert!(
        !said.contains("unknown"),
        "and nothing is reported about a Channel that was never resolved: {said}"
    );
}

// ---- asking rather than installing ---------------------------------------

/// Every other non-zero code Perch has is a refusal, and "there is news" is not
/// one.
#[test]
fn a_check_exits_nought_whether_or_not_there_is_news() {
    for published in [NEWER, upgrade_installed()] {
        let host = machine_with_claude_code()
            .with_reply(LATEST_URL, 200, &latest(published))
            .installed_at("/Users/someone/.local/bin/perch");

        let (outcome, said) = upgrading(
            &host,
            UpgradeArgs {
                check: true,
                ..UpgradeArgs::default()
            },
        );

        assert_eq!(outcome.expect("a check succeeded"), EXIT_OK, "{published}");
        assert!(said.contains(published), "{said}");
        assert!(ran(&host).is_empty(), "a check installs nothing");
    }
}

#[test]
fn a_check_can_answer_as_a_document() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    let (outcome, said) = upgrading(
        &host,
        UpgradeArgs {
            check: true,
            json: true,
            ..UpgradeArgs::default()
        },
    );

    outcome.expect("a check succeeded");
    let document: serde_json::Value = serde_json::from_str(&said).expect("it is a document");
    assert_eq!(document["newest"], NEWER);
    assert_eq!(document["installed"], upgrade_installed());
    assert_eq!(document["channel"], "installer");
    assert_eq!(document["upgrade_available"], true);
}

#[test]
fn the_release_that_is_already_installed_is_nothing_to_do() {
    let host = machine_with_claude_code()
        .with_reply(LATEST_URL, 200, &latest(upgrade_installed()))
        .installed_at("/Users/someone/.local/bin/perch");

    let (outcome, _) = upgrading(&host, UpgradeArgs::default());

    let refused = outcome.expect_err("there is nothing newer");
    assert!(matches!(refused, PerchError::NothingToDo(_)), "{refused}");
    assert!(ran(&host).is_empty());
}

// ---- naming a Release ----------------------------------------------------

#[test]
fn a_release_that_is_not_one_is_refused_without_asking_anybody() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            release: Some("latest".to_string()),
            ..UpgradeArgs::default()
        },
    );

    let refused = outcome.expect_err("`latest` is not a Release");
    assert!(matches!(refused, PerchError::Invalid(_)), "{refused}");
    assert!(host.http_calls().is_empty(), "nothing was asked");
}

#[test]
fn a_named_release_reaches_npm_as_an_install_of_that_version() {
    let host = machine().with_file("/usr/bin/npm", "").installed_at(
        "/usr/lib/node_modules/perch-cli/node_modules/@perch-cli/linux-x64/bin/perch",
    );

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            release: Some(format!("v{NEWER}")),
            ..UpgradeArgs::default()
        },
    );

    outcome.expect("it ran npm");
    assert_eq!(
        ran(&host)[0].1,
        vec![
            "install".to_string(),
            "-g".to_string(),
            format!("perch-cli@{NEWER}")
        ],
        "and the `v` is stripped, because npm never had one"
    );
}

#[test]
fn a_named_release_is_refused_on_homebrew_rather_than_quietly_ignored() {
    let host = with_brew(machine()).installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            release: Some(NEWER.to_string()),
            ..UpgradeArgs::default()
        },
    );

    let refused = outcome.expect_err("Homebrew cannot be pointed at a Release");
    assert!(refused.to_string().contains("Homebrew"), "{refused}");
    assert!(ran(&host).is_empty(), "and nothing was run");
}

/// Nobody is asked to agree to something that is refused whatever they answer,
/// so this refusal comes before the downgrade agreement.
#[test]
fn a_named_release_older_than_this_one_is_refused_before_anybody_agrees_to_it() {
    let host = machine()
        .with_answers(&["y"])
        .installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, said) = upgrading(
        &host,
        UpgradeArgs {
            release: Some(OLDER.to_string()),
            ..UpgradeArgs::default()
        },
    );

    let refused = outcome.expect_err("Homebrew cannot be pointed at a Release");
    assert!(refused.to_string().contains("Homebrew"), "{refused}");
    assert!(
        !said.contains("[y/N]"),
        "nobody is asked to agree to something that is refused either way: {said}"
    );
    assert!(ran(&host).is_empty(), "and nothing was run");
}

/// The same again with no terminal to agree on, where the downgrade refusal
/// would otherwise name `--yes` as a way past a refusal it does not reach.
#[test]
fn a_named_release_on_homebrew_is_refused_as_itself_where_there_is_no_terminal() {
    let host = machine()
        .without_terminal()
        .installed_at("/opt/homebrew/Cellar/perch/0.1.1/bin/perch");

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            release: Some(OLDER.to_string()),
            ..UpgradeArgs::default()
        },
    );

    let refused = outcome.expect_err("Homebrew cannot be pointed at a Release");
    assert!(
        refused.to_string().contains("Homebrew"),
        "the refusal is the one that is actually true, rather than one naming a \
         flag that reaches this same refusal: {refused}"
    );
}

// ---- going backwards -----------------------------------------------------

#[test]
fn an_older_release_is_named_as_one_and_says_what_it_costs() {
    let host = machine()
        .with_answers(&["y"])
        .installed_at("/Users/someone/.local/bin/perch");

    let (outcome, said) = upgrading(
        &host,
        UpgradeArgs {
            release: Some(OLDER.to_string()),
            ..UpgradeArgs::default()
        },
    );

    outcome.expect("they agreed");
    assert!(said.contains("older"), "{said}");
    assert!(
        said.contains("registry"),
        "the consequence is named rather than a bare `are you sure`: {said}"
    );
    assert_eq!(ran(&host).len(), 1);
}

#[test]
fn the_agreement_takes_both_spellings_of_yes_and_nothing_else() {
    for (answer, expected) in [("y", 1), ("yes", 1), ("Y", 1), ("sure", 0), ("", 0)] {
        let host = machine()
            .with_answers(&[answer])
            .installed_at("/Users/someone/.local/bin/perch");

        // Whether it ended in a refusal is the answer's business; what is
        // asserted is whether anything was installed.
        let (_outcome, _said) = upgrading(
            &host,
            UpgradeArgs {
                release: Some(OLDER.to_string()),
                ..UpgradeArgs::default()
            },
        );

        assert_eq!(ran(&host).len(), expected, "answered {answer:?}");
    }
}

#[test]
fn an_older_release_declined_installs_nothing() {
    let host = machine()
        .with_answers(&["n"])
        .installed_at("/Users/someone/.local/bin/perch");

    let (outcome, _) = upgrading(
        &host,
        UpgradeArgs {
            release: Some(OLDER.to_string()),
            ..UpgradeArgs::default()
        },
    );

    outcome.expect_err("they declined");
    assert!(ran(&host).is_empty());
}

#[test]
fn an_older_release_with_nobody_to_ask_is_refused_unless_agreed_ahead_of_time() {
    let refused = machine()
        .without_terminal()
        .installed_at("/Users/someone/.local/bin/perch");
    let (outcome, _) = upgrading(
        &refused,
        UpgradeArgs {
            release: Some(OLDER.to_string()),
            ..UpgradeArgs::default()
        },
    );
    assert!(outcome.is_err(), "nobody could be asked");
    assert!(ran(&refused).is_empty());

    let agreed = machine()
        .without_terminal()
        .installed_at("/Users/someone/.local/bin/perch");
    let (outcome, _) = upgrading(
        &agreed,
        UpgradeArgs {
            release: Some(OLDER.to_string()),
            yes: true,
            ..UpgradeArgs::default()
        },
    );
    outcome.expect("`--yes` answered ahead of time");
    assert_eq!(ran(&agreed).len(), 1);
}

// ---- what an Upgrade is not ----------------------------------------------

/// Which is also why this whole suite arranges no Accounts.
#[test]
fn an_upgrade_touches_nothing_perch_holds() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    upgrading(&host, UpgradeArgs::default())
        .0
        .expect("it ran the installer");

    for effect in host.effects() {
        assert!(
            !matches!(effect, Effect::Took(_)),
            "no lock is taken: {effect:?}"
        );
        assert!(
            !matches!(
                effect,
                Effect::KeychainGet { .. } | Effect::KeychainSet { .. }
            ),
            "no Credential is touched: {effect:?}"
        );
        if let Effect::ReadFile(path) | Effect::WroteFile(path) = &effect {
            assert!(
                !path.to_string_lossy().contains("registry.json"),
                "the registry is not read or written: {effect:?}"
            );
        }
    }
}

// ---- the line `perch --version` adds ------------------------------------

/// `perch --version` is the one place Perch says anything about its own version
/// without being asked, and everything below is the bounding of that.
#[test]
fn a_newer_release_is_mentioned_under_the_version() {
    let host = machine();

    let line = perch::upgrade::notice(&host).expect("there is something newer");

    assert!(line.contains(NEWER), "{line}");
    assert!(
        line.contains("perch upgrade"),
        "it names what to do about it: {line}"
    );
}

/// The first line is spelled exactly as clap spelled it — the Homebrew formula's
/// test block asserts on `perch #{version}` — and the second appears only when
/// there is something to say.
#[test]
fn the_version_report_is_the_version_and_at_most_one_line_more() {
    let quiet =
        machine_with_claude_code().with_reply(LATEST_URL, 200, &latest(upgrade_installed()));
    assert_eq!(
        perch::upgrade::version_report(&quiet),
        format!("perch {}\n", upgrade_installed())
    );

    let behind = perch::upgrade::version_report(&machine());
    let lines: Vec<&str> = behind.lines().collect();
    assert_eq!(lines.len(), 2, "{behind}");
    assert_eq!(lines[0], format!("perch {}", upgrade_installed()));
    assert!(lines[1].contains(NEWER), "{behind}");
    assert!(behind.ends_with('\n'), "{behind:?}");
}

#[test]
fn nothing_is_said_when_nothing_newer_has_been_published() {
    let host = machine_with_claude_code().with_reply(LATEST_URL, 200, &latest(upgrade_installed()));

    assert_eq!(perch::upgrade::notice(&host), None);
}

/// Checked before the request rather than after it: the objection somebody has
/// to this is the network call, not the line.
#[test]
fn the_variable_that_switches_it_off_stops_the_request_rather_than_the_line() {
    let host = machine().with_env(perch::upgrade::NO_CHECK, "1");

    assert_eq!(perch::upgrade::notice(&host), None);
    assert!(
        host.http_calls().is_empty(),
        "nothing went out: {:?}",
        host.http_calls()
    );
}

/// A script parsing `perch --version` and the Homebrew formula's test block both
/// read this output.
#[test]
fn nothing_is_said_and_nothing_is_asked_when_there_is_no_terminal() {
    let host = machine().without_terminal();

    assert_eq!(perch::upgrade::notice(&host), None);
    assert!(host.http_calls().is_empty());
}

#[test]
fn a_machine_that_cannot_answer_loses_the_line_and_nothing_else() {
    // A FakeHost with no arranged reply is a machine with no network.
    let offline = machine_with_claude_code();
    assert_eq!(perch::upgrade::notice(&offline), None);

    let rate_limited = machine_with_claude_code().with_reply(LATEST_URL, 403, "{}");
    assert_eq!(perch::upgrade::notice(&rate_limited), None);

    let nonsense = machine_with_claude_code().with_reply(LATEST_URL, 200, "not json");
    assert_eq!(perch::upgrade::notice(&nonsense), None);

    let tagless = machine_with_claude_code().with_reply(LATEST_URL, 200, r#"{"name":"whatever"}"#);
    assert_eq!(perch::upgrade::notice(&tagless), None);
}

/// Abandoned long before the thirty seconds a Refresh is allowed: the line is
/// worth a pause nobody notices and not a wait they do.
#[test]
fn the_check_is_given_a_short_bound_where_a_refresh_is_not() {
    let host = machine();

    perch::upgrade::notice(&host).expect("there is something newer");

    let asked = host.sent_to(LATEST_URL);
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].within_millis,
        Some(perch::upgrade::CHECK_WITHIN_MILLIS)
    );
}

/// Where a check somebody typed is not bounded that way: they asked, and are
/// waiting for the answer rather than for something else.
#[test]
fn a_check_somebody_asked_for_is_not_cut_short() {
    let host = machine().installed_at("/Users/someone/.local/bin/perch");

    upgrading(
        &host,
        UpgradeArgs {
            check: true,
            ..UpgradeArgs::default()
        },
    )
    .0
    .expect("a check succeeded");

    assert_eq!(host.sent_to(LATEST_URL)[0].within_millis, None);
}

fn upgrade_installed() -> &'static str {
    perch::upgrade::installed()
}
