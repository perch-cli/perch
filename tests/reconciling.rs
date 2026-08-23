//! Behavior tests for Reconcile: making Shared State reachable from the
//! Profile a Run launches (ADR everything-but-the-account).
//!
//! Driven through the Host port, so the Windows half — junctions, and the
//! symbolic link privilege a machine without Developer Mode does not have —
//! is asserted from whatever machine the tests run on.

// Every path compared here comes out of the fake's effect log, spelled as the
// code under test wrote it: filtering that log by prefix asks which effects
// landed under a directory, and never whether a path on a machine is inside one.
#![allow(
    clippy::disallowed_methods,
    reason = "the fake's effect log, filtered by the prefix it was written under"
)]

use std::path::{Path, PathBuf};

use perch::host::fake::Effect;
use perch::host::prelude::*;
use perch::host::{FakeHost, Link, PRIVATE_DIR_MODE, Platform};
use perch::reconcile::reconcile;

/// The Default Profile: the configuration directory Claude Code falls back to,
/// holding everything that belongs to the person.
const SHARED: &str = "/Users/someone/.claude";

/// The Profile a Run would be launched against.
const PROFILE: &str = "/Users/someone/.config/perch/profiles/someone-example-com";

/// A machine whose Default Profile holds the spread a real one does: memory,
/// settings, plugins, plans — and the entries that stay behind, the two that
/// belong to the Account and the one that belongs to the directory itself.
fn machine() -> FakeHost {
    FakeHost::new()
        .with_file(shared("CLAUDE.md"), "remember this")
        .with_file(shared("settings.json"), r#"{"theme":"dark"}"#)
        .with_file(shared("plugins/config.json"), "{}")
        .with_file(shared("sessions/4321.json"), "{}")
        .with_file(shared("plans/one.md"), "a plan")
        .with_file(shared(".credentials.json"), r#"{"claudeAiOauth":{}}"#)
        .with_file(shared(".claude.json"), r#"{"oauthAccount":{}}"#)
}

fn shared(entry: &str) -> String {
    format!("{SHARED}/{entry}")
}

fn profile(entry: &str) -> String {
    format!("{PROFILE}/{entry}")
}

/// Runs the pass a Run would run.
fn run_reconcile(host: &FakeHost) -> perch::Result<()> {
    reconcile(host, Path::new(SHARED), Path::new(PROFILE))
}

/// What a Profile now holds, by entry name, whether it is a link or not.
fn entries_of(host: &FakeHost) -> Vec<String> {
    host.list_dir(Path::new(PROFILE))
        .expect("the Profile is there")
        .iter()
        .filter_map(|path| path.file_name()?.to_str().map(str::to_string))
        .collect()
}

/// What one entry of the Profile stands for, and how.
fn share_of(host: &FakeHost, entry: &str) -> (Link, PathBuf) {
    host.link_at(profile(entry))
        .unwrap_or_else(|| panic!("`{entry}` is shared into the Profile"))
}

#[test]
fn every_entry_of_the_default_profile_crosses_except_the_three_that_stay_behind() {
    let host = machine();

    run_reconcile(&host).expect("everything can be linked");

    assert_eq!(
        entries_of(&host),
        vec!["CLAUDE.md", "plans", "plugins", "settings.json"],
    );
    assert_eq!(
        host.link_at(profile(".credentials.json")),
        None,
        "the Credential is the Account's, and stays in the Profile it belongs to"
    );
    assert_eq!(
        host.link_at(profile(".claude.json")),
        None,
        "the file naming the Account is per-Profile and cannot be shared"
    );
}

/// The third entry that stays behind, neither the person's nor the Account's but
/// the directory's own: shared, one client's Marker would be the answer for
/// every Profile at once.
#[test]
fn the_directory_that_records_who_is_running_does_not_cross() {
    let host = machine();

    run_reconcile(&host).expect("everything can be linked");

    assert_eq!(
        host.link_at(profile("sessions")),
        None,
        "a Profile answers for its own clients and nobody else's"
    );
    assert!(
        !entries_of(&host).contains(&"sessions".to_string()),
        "{:?}",
        entries_of(&host)
    );
}

/// The fourth, on the same rule — and the only one of Claude Code's three locks
/// that sits inside the directory, so the only one a Reconcile ever sees. It is
/// only there at all while somebody holds it, which is what makes this rare.
#[test]
fn the_lock_a_credential_is_renewed_under_does_not_cross() {
    let held = machine();
    let now = held.now();
    let host = held.with_dir_held_since(shared(".oauth_refresh.lock"), now);

    run_reconcile(&host).expect("everything else can be linked");

    assert_eq!(
        host.link_at(profile(".oauth_refresh.lock")),
        None,
        "a Profile takes its own refresh lock and never the Default Profile's"
    );
    assert!(
        !entries_of(&host).contains(&".oauth_refresh.lock".to_string()),
        "{:?}",
        entries_of(&host)
    );
}

#[test]
fn an_entry_perch_has_never_heard_of_crosses_without_a_code_change() {
    let host = machine()
        .with_file(shared("telemetry-v3/spool.bin"), "")
        .with_file(shared("whatever-comes-next.json"), "{}");

    run_reconcile(&host).expect("everything can be linked");

    assert_eq!(
        share_of(&host, "telemetry-v3").1,
        PathBuf::from(shared("telemetry-v3"))
    );
    assert_eq!(
        share_of(&host, "whatever-comes-next.json").1,
        PathBuf::from(shared("whatever-comes-next.json"))
    );
}

#[test]
fn off_windows_every_share_is_a_symbolic_link() {
    let host = machine().with_platform(Platform::Other);

    run_reconcile(&host).expect("everything can be linked");

    for entry in ["CLAUDE.md", "settings.json", "plugins", "plans"] {
        assert_eq!(
            share_of(&host, entry),
            (Link::Symbolic, PathBuf::from(shared(entry))),
            "{entry}"
        );
    }
}

#[test]
fn windows_without_developer_mode_uses_junctions_and_hard_links() {
    let host = machine().with_platform(Platform::Windows);

    run_reconcile(&host).expect("a Windows without the privilege still shares");

    for directory in ["plugins", "plans"] {
        assert_eq!(
            share_of(&host, directory).0,
            Link::Junction,
            "{directory} is a directory"
        );
    }
    for file in ["CLAUDE.md", "settings.json"] {
        assert_eq!(share_of(&host, file).0, Link::Hard, "{file} is a file");
    }
}

#[test]
fn windows_with_developer_mode_prefers_a_symbolic_link_for_a_file() {
    let host = machine()
        .with_platform(Platform::Windows)
        .with_developer_mode();

    run_reconcile(&host).expect("everything can be linked");

    assert_eq!(share_of(&host, "CLAUDE.md").0, Link::Symbolic);
    assert_eq!(
        share_of(&host, "plugins").0,
        Link::Junction,
        "a junction is what a directory gets on Windows either way"
    );
}

#[test]
fn nothing_is_ever_copied_into_the_profile() {
    for platform in [Platform::MacOs, Platform::Windows, Platform::Other] {
        let host = machine().with_platform(platform);
        host.forget_effects();

        run_reconcile(&host).expect("everything can be linked");

        let copied: Vec<Effect> = host
            .effects()
            .into_iter()
            .filter(|effect| {
                matches!(effect, Effect::WroteFile(path) | Effect::WrotePrivateFile(path)
                    if path.starts_with(PROFILE))
            })
            .collect();
        assert!(copied.is_empty(), "{platform:?} copied: {copied:?}");
    }
}

#[test]
fn a_link_that_cannot_be_made_refuses_and_names_the_entry_and_the_reason() {
    let host = machine().with_unwritable_file(profile("plugins"), "Read-only file system");

    let refusal = run_reconcile(&host).expect_err("the Run does not launch");

    let said = refusal.to_string();
    assert!(said.contains("`plugins`"), "{said}");
    assert!(said.contains("Read-only file system"), "{said}");
    assert!(said.contains("never by copying"), "{said}");
    assert_eq!(
        host.file(profile("plugins")),
        None,
        "a refusal leaves nothing behind, least of all a copy"
    );
}

#[test]
fn a_windows_refusal_says_what_it_tried_and_what_would_let_it_through() {
    let host = machine()
        .with_platform(Platform::Windows)
        .with_unwritable_file(profile("CLAUDE.md"), "The device is not ready");

    let said = run_reconcile(&host)
        .expect_err("the Run does not launch")
        .to_string();

    assert!(said.contains("a symbolic link could not be made"), "{said}");
    assert!(said.contains("a hard link could not be made"), "{said}");
    assert!(said.contains("Developer Mode"), "{said}");
}

#[test]
fn a_link_pointing_somewhere_stale_is_repaired_rather_than_left() {
    let host = machine().with_link(
        Link::Symbolic,
        "/Users/someone/.config/perch/profiles/somebody-else/plugins",
        profile("plugins"),
    );

    run_reconcile(&host).expect("the stale link is replaced");

    assert_eq!(
        share_of(&host, "plugins"),
        (Link::Symbolic, PathBuf::from(shared("plugins")))
    );
}

#[test]
fn a_link_to_an_entry_that_has_gone_is_taken_away() {
    let host = machine().with_link(
        Link::Symbolic,
        shared(".oauth_refresh.lock"),
        profile(".oauth_refresh.lock"),
    );

    run_reconcile(&host).expect("the broken link is swept up");

    assert_eq!(host.link_at(profile(".oauth_refresh.lock")), None);
    assert!(!entries_of(&host).contains(&".oauth_refresh.lock".to_string()));
}

#[test]
fn a_broken_link_that_is_not_a_share_of_the_default_profile_is_left_alone() {
    let host = machine().with_link(Link::Symbolic, "/Volumes/backup/notes", profile("notes"));

    run_reconcile(&host).expect("everything else can be linked");

    assert_eq!(
        host.link_at(profile("notes")),
        Some((Link::Symbolic, PathBuf::from("/Volumes/backup/notes"))),
        "a Profile is not Perch's to tidy"
    );
}

#[test]
fn a_second_pass_over_an_unchanged_machine_touches_nothing() {
    let host = machine();
    run_reconcile(&host).expect("the first pass links everything");
    host.forget_effects();

    run_reconcile(&host).expect("the second pass has nothing to do");

    let touched: Vec<Effect> = host
        .effects()
        .into_iter()
        .filter(|effect| {
            matches!(
                effect,
                Effect::Linked { .. } | Effect::RemovedLink(_) | Effect::RemovedFile(_)
            )
        })
        .collect();
    assert!(touched.is_empty(), "{touched:?}");
}

/// An editor that writes beside a file and renames it into place leaves the
/// Default Profile holding a new file and the Profile still naming the old one,
/// which is what re-establishing before every Run keeps down to one Run.
#[test]
fn a_hard_linked_file_is_re_established_every_pass() {
    let host = machine().with_platform(Platform::Windows);
    run_reconcile(&host).expect("the first pass links everything");
    assert_eq!(share_of(&host, "CLAUDE.md").0, Link::Hard);

    // A replacement from outside: the same name, a different file behind it.
    host.set_file(shared("CLAUDE.md"), "remember this instead");
    host.forget_effects();
    run_reconcile(&host).expect("the second pass re-establishes it");

    assert_eq!(
        host.file(profile("CLAUDE.md")).as_deref(),
        Some("remember this instead"),
        "a Run reads what the Default Profile actually holds"
    );
    assert!(
        host.effects().iter().any(|effect| matches!(
            effect,
            Effect::Linked {
                kind: Link::Hard,
                ..
            }
        )),
        "the hard link is made again rather than believed"
    );
}

#[test]
fn something_that_is_not_a_link_in_the_way_is_refused_rather_than_deleted() {
    let host = machine().with_file(profile("CLAUDE.md"), "somebody's own file");

    let said = run_reconcile(&host)
        .expect_err("the Run does not launch")
        .to_string();

    assert!(said.contains("`CLAUDE.md`"), "{said}");
    assert!(said.contains("is not a link Perch made"), "{said}");
    assert_eq!(
        host.file(profile("CLAUDE.md")).as_deref(),
        Some("somebody's own file")
    );
    // This is a path to move rather than a privilege to turn on, and the
    // remedy has to be the one that fits.
    assert!(said.contains("move it aside or remove it"), "{said}");
    assert!(!said.contains("filesystem that carries no links"), "{said}");
}

#[test]
fn the_profiles_own_credential_and_identity_are_untouched() {
    let host = machine()
        .with_file(
            profile(".credentials.json"),
            r#"{"claudeAiOauth":{"mine":1}}"#,
        )
        .with_file(profile(".claude.json"), r#"{"oauthAccount":{"mine":1}}"#);

    run_reconcile(&host).expect("everything can be linked");

    assert_eq!(
        host.file(profile(".credentials.json")).as_deref(),
        Some(r#"{"claudeAiOauth":{"mine":1}}"#)
    );
    assert_eq!(
        host.file(profile(".claude.json")).as_deref(),
        Some(r#"{"oauthAccount":{"mine":1}}"#)
    );
    assert_eq!(host.link_at(profile(".credentials.json")), None);
}

#[test]
fn a_profile_that_is_not_there_yet_is_created_for_its_owner_alone() {
    let host = machine();
    assert!(!host.path_exists(Path::new(PROFILE)));

    run_reconcile(&host).expect("everything can be linked");

    assert_eq!(host.mode_of(PROFILE), Some(PRIVATE_DIR_MODE));
}

#[test]
fn a_default_profile_that_is_not_there_shares_nothing() {
    let host = FakeHost::new();

    run_reconcile(&host).expect("there is simply nothing to do");

    assert_eq!(
        host.list_dir(Path::new(PROFILE))
            .expect("the Profile itself is made")
            .len(),
        0
    );
}

/// A `CLAUDE_CONFIG_DIR` that puts a Profile inside the Default Profile:
/// everything else crosses, and the Profile is not linked into itself.
#[test]
fn a_profile_inside_the_default_profile_is_not_linked_into_itself() {
    let nested = shared("nested");
    let host = machine().with_file(format!("{nested}/.credentials.json"), "{}");

    reconcile(&host, Path::new(SHARED), Path::new(&nested)).expect("everything else crosses");

    assert_eq!(host.link_at(format!("{nested}/nested")), None);
    assert_eq!(
        host.link_at(format!("{nested}/CLAUDE.md")),
        Some((Link::Symbolic, PathBuf::from(shared("CLAUDE.md"))))
    );
}

/// `PERCH_HOME=~/.claude/perch` under `CLAUDE_CONFIG_DIR=~/.claude` is the
/// arrangement that reaches this: the entry that crosses is `~/.claude/perch`
/// and the Profile sits three levels inside it, so the two are different paths.
#[test]
fn an_entry_that_contains_the_profile_is_not_linked_into_it_however_deep_it_is() {
    let inside = format!("{SHARED}/perch/profiles/someone-example-com");
    let host = machine().with_file(shared("perch/registry.json"), "{}");

    reconcile(&host, Path::new(SHARED), Path::new(&inside)).expect("everything else crosses");

    assert_eq!(
        host.link_at(format!("{inside}/perch")),
        None,
        "a link here holds the Profile it was made in"
    );
    assert_eq!(
        host.link_at(format!("{inside}/CLAUDE.md")),
        Some((Link::Symbolic, PathBuf::from(shared("CLAUDE.md"))))
    );
}

/// The same where the containment is only true after a link is followed: a
/// `~/.claude/perch` that is a *link* to `~/.config/perch` is two different
/// strings from the Profile under it. One hop is all a dotfile manager makes.
#[test]
fn an_entry_that_only_holds_the_profile_once_its_link_is_followed_is_not_linked_either() {
    let host = machine()
        .with_file("/Users/someone/.config/perch/registry.json", "{}")
        .with_link(
            Link::Symbolic,
            "/Users/someone/.config/perch",
            shared("perch"),
        );

    reconcile(&host, Path::new(SHARED), Path::new(PROFILE)).expect("everything else crosses");

    assert_eq!(
        host.link_at(profile("perch")),
        None,
        "following it lands above the Profile, so the link would hold what made it"
    );
    assert_eq!(
        host.link_at(profile("CLAUDE.md")),
        Some((Link::Symbolic, PathBuf::from(shared("CLAUDE.md")))),
        "and the entries that are nothing to do with it still cross"
    );
}

/// And where the link is on the *Profile*'s side of the question. A
/// `PERCH_HOME` reached through one — `~/perch-data`, linked at `~/.claude/perch`
/// — spells the Profile a second way, and only resolving that spelling says the
/// entry crossing holds it.
#[test]
fn an_entry_that_holds_the_profile_under_its_other_spelling_is_not_linked_either() {
    let host = machine()
        .with_file(shared("perch/registry.json"), "{}")
        .with_link(Link::Symbolic, "/Users/someone/perch-data", shared("perch"));

    let into = "/Users/someone/perch-data/profiles/someone-example-com";
    reconcile(&host, Path::new(SHARED), Path::new(into)).expect("everything else crosses");

    assert_eq!(
        host.link_at(format!("{into}/perch")),
        None,
        "the Profile is inside the entry under the name Perch was handed, so a \
         link here points at a directory holding the Profile that made it"
    );
    assert_eq!(
        host.link_at(format!("{into}/CLAUDE.md")),
        Some((Link::Symbolic, PathBuf::from(shared("CLAUDE.md")))),
        "and the entries that are nothing to do with it still cross"
    );
}

#[test]
fn the_default_profile_is_never_reconciled_into_itself() {
    let host = machine();
    host.forget_effects();

    reconcile(&host, Path::new(SHARED), Path::new(SHARED)).expect("nothing to do");

    assert!(
        !host
            .effects()
            .iter()
            .any(|effect| matches!(effect, Effect::Linked { .. })),
        "{:?}",
        host.effects()
    );
}

/// The denylist is enforced where links are *made*, so nothing else ever looks
/// at one already sitting at a held-back name and it would stay for good — and a
/// Profile whose Credential Store is a link into the Default Profile has no
/// Credential of its own.
#[test]
fn a_link_at_a_held_back_name_is_taken_away_even_though_its_target_is_there() {
    let host = machine()
        .with_link(
            Link::Symbolic,
            shared(".credentials.json"),
            profile(".credentials.json"),
        )
        .with_link(Link::Symbolic, shared("sessions"), profile("sessions"));

    run_reconcile(&host).expect("the links are swept up");

    assert!(
        host.link_at(profile(".credentials.json")).is_none(),
        "a Profile's Credential Store is never the Default Profile's"
    );
    assert!(
        host.link_at(profile("sessions")).is_none(),
        "and its sessions are never every other Profile's"
    );
    assert!(
        !entries_of(&host)
            .iter()
            .any(|entry| entry == ".credentials.json"),
        "{:?}",
        entries_of(&host)
    );
}

/// What refuses a `remove_link` is the directory holding it — a Profile left
/// root-owned by a `sudo claude` is how that happens — and Developer Mode has
/// nothing to say about it, so it must not be the remedy named.
#[test]
fn a_link_that_cannot_be_taken_away_names_the_directory_rather_than_developer_mode() {
    let stale = profile("plugins");
    let host = machine()
        .with_link(
            Link::Symbolic,
            "/Users/someone/.config/perch/profiles/somebody-else/plugins",
            &stale,
        )
        .with_undeletable_file(&stale, "Permission denied (os error 13)");

    let said = run_reconcile(&host)
        .expect_err("the stale link cannot be replaced")
        .to_string();

    assert!(said.contains("could not be replaced"), "{said}");
    assert!(
        !said.contains("Developer Mode") && !said.contains("filesystem that carries no links"),
        "making a link is not what failed, so it is not what the repair is \
         about: {said}"
    );
    assert!(
        said.contains("the directory holding it"),
        "what actually refused is named: {said}"
    );
}

/// Both platforms, because they fail differently and only one of them says so:
/// everywhere else the Run is refused naming the person's own file as the
/// obstruction, and a Windows share is routinely a hard link, so `in_the_way`
/// deletes what is there and reports a Run that worked.
#[test]
fn a_profile_linked_at_the_default_profile_is_already_holding_all_of_it() {
    for (platform, kind) in [
        (Platform::MacOs, Link::Symbolic),
        (Platform::Windows, Link::Junction),
    ] {
        let host = machine().with_platform(platform);
        host.link(kind, Path::new(SHARED), Path::new(PROFILE))
            .expect("the Profile is linked at the Default Profile");

        run_reconcile(&host).unwrap_or_else(|refused| {
            panic!("[{platform:?}] the Profile already holds all of it: {refused}")
        });

        assert_eq!(
            host.read_file(Path::new(&shared("settings.json")))
                .ok()
                .as_deref(),
            Some(r#"{"theme":"dark"}"#),
            "[{platform:?}] and nothing of the person's was replaced by a link \
             to itself"
        );
    }
}
