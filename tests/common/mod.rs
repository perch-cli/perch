//! Fixtures for the behavior tests: a machine with a Claude Code login on it.
//!
//! Where a test lives is decided by what it names
//! (ADR a-suite-is-named-and-gated): a `mod tests` in `src` asserts a module's
//! own vocabulary through the module's own API, a binary in `tests/` asserts
//! what a *command* does. The fake is not the discriminator — `src/lock.rs`
//! and `src/registry.rs` both drive `FakeHost` from inside their own
//! `mod tests`.

// Each binary gets its own copy of this module and uses the part it needs, so
// an unused fixture here is the normal case rather than rot.
#![allow(dead_code)]

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use perch::anthropic::{PROFILE_URL, USAGE_URL};
use perch::commands::add::AddArgs;
use perch::commands::alias::AliasCommand;
use perch::commands::config::ConfigCommand;
use perch::commands::enable::EnableCommand;
use perch::commands::group::GroupCommand;
use perch::commands::list::ListArgs;
use perch::commands::relogin::ReloginArgs;
use perch::commands::remove::RemoveArgs;
use perch::commands::run::RunArgs;
use perch::commands::status::StatusArgs;
use perch::commands::switch::SwitchArgs;
use perch::commands::watcher::WatcherCommand;
use perch::credentials;
use perch::host::fake::THIS_PROCESS;
use perch::host::prelude::*;
use perch::host::{Execution, FakeHost, Platform};
use perch::probe;
use perch::registry::{CachedUtilization, Quarantine, WindowUtilization};

pub const CLAUDE_VERSION: &str = "2.1.221";
pub const LOGIN_NAME: &str = "someone";
pub const EMAIL: &str = "someone@example.com";
pub const DEFAULT_SERVICE: &str = "Claude Code-credentials";
pub const REGISTRY_PATH: &str = "/Users/someone/.config/perch/registry.json";
pub const IDENTITY_PATH: &str = "/Users/someone/.claude.json";

pub const CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-test","refreshToken":"sk-ant-ort01-test","expiresAt":1785000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"pro"}}"#;

/// The Account a second login produces: a different person, a different
/// organization, a different plan.
pub const SECOND_EMAIL: &str = "overflow@example.com";
pub const SECOND_CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-second","refreshToken":"sk-ant-ort01-second","expiresAt":1785000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"max"}}"#;

pub const SECOND_IDENTITY_FILE: &str = r#"{
  "numStartups": 1,
  "oauthAccount": {
    "accountUuid": "account-uuid-2",
    "emailAddress": "overflow@example.com",
    "organizationUuid": "organization-uuid-2",
    "organizationName": "Overflow Ltd",
    "organizationRole": "admin"
  },
  "projects": {}
}"#;

/// A third Account, for the cases that need more than two.
pub const THIRD_EMAIL: &str = "spare@example.com";
pub const THIRD_CREDENTIAL: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-third","refreshToken":"sk-ant-ort01-third","expiresAt":1785000000000,"scopes":["user:inference","user:profile"],"subscriptionType":"pro"}}"#;
pub const THIRD_TOKEN: &str = "sk-ant-oat01-third";

pub const THIRD_IDENTITY_FILE: &str = r#"{
  "numStartups": 1,
  "oauthAccount": {
    "accountUuid": "account-uuid-3",
    "emailAddress": "spare@example.com",
    "organizationUuid": "organization-uuid-3",
    "organizationName": "Spare Ltd",
    "organizationRole": "admin"
  },
  "projects": {}
}"#;

pub const IDENTITY_FILE: &str = r#"{
  "numStartups": 41,
  "oauthAccount": {
    "accountUuid": "account-uuid-1",
    "emailAddress": "someone@example.com",
    "organizationUuid": "organization-uuid-1",
    "organizationName": "Acme",
    "organizationRole": "admin"
  },
  "projects": {}
}"#;

/// Where the fixtures install Claude Code — the path `claude` resolves to
/// when PATH is walked.
pub const CLAUDE_BIN: &str = "/usr/bin/claude";

/// A machine with Claude Code installed but nobody logged in.
pub fn machine_with_claude_code() -> FakeHost {
    FakeHost::new()
        .with_env("PATH", "/usr/bin")
        .with_file(CLAUDE_BIN, "")
        .with_exec(
            CLAUDE_BIN,
            &["--version"],
            Execution {
                status: 0,
                stdout: format!("{CLAUDE_VERSION} (Claude Code)\n"),
                stderr: String::new(),
            },
        )
}

/// A machine with Claude Code installed and logged in — what everyone
/// installing Perch actually has.
pub fn logged_in_machine() -> FakeHost {
    machine_with_claude_code()
        .with_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, CREDENTIAL)
        .with_file("/Users/someone/.claude.json", IDENTITY_FILE)
}

/// A machine that is not a Mac: Claude Code installed and logged in, keeping
/// its Credential in the file that is the store there.
pub fn logged_in_machine_off_macos() -> FakeHost {
    machine_with_claude_code()
        .with_platform(Platform::Other)
        .with_file(CREDENTIALS_PATH, CREDENTIAL)
        .with_file(IDENTITY_PATH, IDENTITY_FILE)
}

/// The plaintext Credential Store of the default config directory.
pub const CREDENTIALS_PATH: &str = "/Users/someone/.claude/.credentials.json";

/// A login that completes, leaving behind exactly what Claude Code would in
/// the config directory it was pointed at: the Credential in whichever store
/// that platform's Claude Code writes to, and the Identity in its
/// `.claude.json`.
pub fn login_producing(
    credential: &'static str,
    identity_file: &'static str,
) -> impl Fn(&FakeHost, &Path) -> i32 {
    move |host, dir| {
        let store = probe::store_for_profile(host, dir).expect("USER is set");
        let [primary, _] = credentials::stores_for(host, &store);
        primary
            .write(host, credential)
            .expect("the login stores what it produced");
        host.set_file(dir.join(".claude.json"), identity_file);
        0
    }
}

/// A login the user walked away from: it writes nothing and exits non-zero.
pub fn abandoned_login() -> impl Fn(&FakeHost, &Path) -> i32 {
    |_host, _dir| 1
}

/// The client a Run launches, standing where a login stands for `add`: it is
/// handed a Profile that is already an Account's, so it writes nothing of its
/// own and only exits with the status the test is about.
pub fn client_exiting(status: i32) -> impl Fn(&FakeHost, &Path) -> i32 {
    move |_host, _dir| status
}

/// A `perch run` against an Account, as far as the rest of Perch can see one:
/// the marker Run wrote into the Profile (ADR a-run-is-one-shot). `began` now
/// is a Run still going; an hour ago, against a process since replaced, is the
/// marker a killed Run left behind.
pub fn a_run_against(host: &FakeHost, email: &str, began: DateTime<Utc>) {
    let profile = perch::holdings::profile_dir_for(host, email).expect("home is known");
    host.set_file(
        probe::session_marker_at(&profile, THIS_PROCESS),
        &probe::session_marker(THIS_PROCESS, began),
    );
}

/// The marker a Claude Code *client* writes, deliberately not
/// [`probe::session_marker`], which is a Run's. The probe reads `startedAt` and
/// ignores the rest whoever left it (ADR a-profile-is-live-by-evidence), so a
/// fixture writing Perch's marker for a client stops testing that. The path is
/// `probe`'s: where a marker lives is Claude Code's convention, not this file's.
pub fn a_client_marker(pid: u32, began: DateTime<Utc>) -> String {
    format!(
        r#"{{"pid":{pid},"cwd":"/Users/someone/work","startedAt":{}}}"#,
        began.timestamp_millis()
    )
}

/// A client running against `config_dir` right now: the marker it wrote, and a
/// process still behind it. Takes the machine by reference, as [`a_run_against`]
/// does, so a fixture arranged inside a login or a wait can reach it too.
pub fn a_client_running_against(host: &FakeHost, config_dir: impl AsRef<Path>, pid: u32) {
    host.set_file(
        probe::session_marker_at(config_dir.as_ref(), pid),
        &a_client_marker(pid, host.now()),
    );
    host.set_live_process(pid);
}

/// One command run against the machine, with what it printed decoded. The
/// writer and the UTF-8 read live here, so a `run_*` wrapper is only the
/// arguments it names.
pub fn ran<T>(
    host: &FakeHost,
    command: impl FnOnce(&FakeHost, &mut Vec<u8>) -> perch::Result<T>,
) -> (perch::Result<T>, String) {
    let mut written = Vec::new();
    let result = command(host, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// Runs `perch run <target>`, returning the status the client exited with — or
/// Perch's refusal to launch one — alongside what was printed.
pub fn run_run(host: &FakeHost, target: &str) -> (perch::Result<i32>, String) {
    run_run_with(host, target, &[])
}

/// The same, with the words somebody typed after `--`.
pub fn run_run_with(
    host: &FakeHost,
    target: &str,
    command: &[&str],
) -> (perch::Result<i32>, String) {
    let args = RunArgs {
        target: target.to_string(),
        command: command.iter().map(|word| word.to_string()).collect(),
    };
    ran(host, |host, written| {
        perch::commands::run::run(host, args, written)
    })
}

pub fn run_status(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_status_with(
        host,
        StatusArgs {
            json,
            ..StatusArgs::default()
        },
    )
}

/// `perch status --refresh`: the one command that fetches about the Account you
/// are on (ADR a-figure-carries-its-age).
pub fn run_status_refresh(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_status_with(
        host,
        StatusArgs {
            json,
            refresh: true,
        },
    )
}

pub fn run_status_with(host: &FakeHost, args: StatusArgs) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::status::run(host, args, written)
    })
}

pub fn run_list(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_list_with(
        host,
        ListArgs {
            json,
            ..ListArgs::default()
        },
    )
}

/// `perch list --refresh`: fetch for every Account Perch holds.
pub fn run_list_refresh(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_list_with(
        host,
        ListArgs {
            json,
            refresh: true,
            ..ListArgs::default()
        },
    )
}

/// `perch list <scope>`: the same cached view, narrowed to one Group or to the
/// Accounts in no Group.
pub fn run_list_in(host: &FakeHost, scope: &str, json: bool) -> (perch::Result<()>, String) {
    run_list_with(
        host,
        ListArgs {
            json,
            scope: Some(scope.to_string()),
            ..ListArgs::default()
        },
    )
}

/// `perch list <scope> --refresh`: fetch for every Account you could land on.
pub fn run_list_in_refresh(
    host: &FakeHost,
    scope: &str,
    json: bool,
) -> (perch::Result<()>, String) {
    run_list_with(
        host,
        ListArgs {
            json,
            scope: Some(scope.to_string()),
            refresh: true,
        },
    )
}

pub fn run_list_with(host: &FakeHost, args: ListArgs) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::list::run(host, args, written)
    })
}

/// A machine holding two Accounts, neither in a Group and neither named: the
/// ordinary starting state once a second Account has been added
/// (ADR a-group-is-a-declaration).
pub fn machine_with_two_accounts() -> FakeHost {
    let host =
        logged_in_machine().with_login(login_producing(SECOND_CREDENTIAL, SECOND_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");
    host
}

/// A machine holding three Accounts, none of them in a Group: what the cases
/// that need more than a pair start from.
pub fn machine_with_three_accounts() -> FakeHost {
    let host = machine_with_two_accounts()
        .with_login(login_producing(THIRD_CREDENTIAL, THIRD_IDENTITY_FILE));
    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the third Account is added");
    host
}

/// A machine holding three Accounts, all in one Group, the first one active.
/// The ordinary shape of the problem Cycling exists for: several subscriptions
/// declared interchangeable, one of them running dry.
pub fn three_accounts_in_one_group() -> FakeHost {
    let host = machine_with_three_accounts();
    a_group_of(&host, "work", &[EMAIL, SECOND_EMAIL, THIRD_EMAIL]);
    host
}

pub fn run_add(host: &FakeHost, args: AddArgs) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::add::run(host, args, written)
    })
}

/// Where an Account's Profile keeps its things, derived the way every command
/// derives it now that nothing records it (ADR claude-code-chooses-the-store).
pub fn store_of(host: &FakeHost, email: &str) -> probe::Store {
    let dir = perch::holdings::profile_dir_for(host, email).expect("home is known");
    probe::store_for_profile(host, &dir).expect("USER is set")
}

/// The Credential a Profile holds, from whichever of its two Credential Stores
/// holds one — which is the only question a test about a stored Credential
/// should be asking.
pub fn credential_of(host: &FakeHost, email: &str) -> Option<String> {
    perch::credentials::read(host, &store_of(host, email))
        .expect("the store could be consulted")
        .map(|held| held.credential.to_string())
}

/// The Registry as it would be read back by the next command Perch runs.
pub fn registry_of(host: &FakeHost) -> perch::registry::Registry {
    perch::registry::load(host)
        .unwrap()
        .expect("a registry is written")
}

/// Writes the Registry the way a command does, which means under the lock a
/// command holds. Taken and given back here, so a fixture arranging a state is
/// one line rather than three.
pub fn save_registry(host: &FakeHost, registry: &perch::registry::Registry) {
    let mut perch = perch::holdings::lock(host).expect("the registry lock is free");
    // Cloned rather than taken by `&mut`: `save` stamps the version into what it
    // is handed, and a fixture's caller keeps reading the copy it built.
    let mut written = registry.clone();
    perch::registry::save(host, &mut perch, &mut written).expect("the registry is written");
}

/// What a Perch killed mid-Switch leaves on the registry: a Landing naming the
/// Account it was leaving and the one it was switching to
/// (ADR a-switch-is-written-down-first). The Registry half only — what the
/// machine holds is what a Landing is settled against, so each test arranges
/// that half itself.
pub fn a_switch_died_mid_flight(host: &FakeHost, leaving: Option<&str>, arriving: &str) {
    let mut registry = registry_of(host);
    registry.begin_landing(leaving.map(str::to_string), arriving);
    save_registry(host, &registry);
}

/// `perch group add <name>`, for the tests that only need the Group to exist.
pub fn declare_group(host: &FakeHost, name: &str) {
    run_group(
        host,
        GroupCommand::Add {
            name: name.to_string(),
        },
    )
    .0
    .unwrap_or_else(|err| panic!("could not declare `{name}`: {err}"));
}

/// A Group with these Accounts in it: the result, rather than the two commands
/// that produce it. Panics naming the Account that would not join, a failure
/// inside the loop being otherwise a message about no Account in particular.
pub fn a_group_of(host: &FakeHost, name: &str, accounts: &[&str]) {
    declare_group(host, name);
    for account in accounts {
        move_to_group(host, account, name)
            .0
            .unwrap_or_else(|err| panic!("`{account}` could not join `{name}`: {err}"));
    }
}

pub fn move_to_group(host: &FakeHost, target: &str, group: &str) -> (perch::Result<()>, String) {
    run_group(
        host,
        GroupCommand::Move {
            target: target.to_string(),
            group: group.to_string(),
        },
    )
}

pub fn run_group(host: &FakeHost, command: GroupCommand) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::group::run(host, command, written)
    })
}

pub fn run_switch(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_switch_with(
        host,
        SwitchArgs {
            target: Some(target.to_string()),
            no_refresh: false,
        },
    )
}

/// `perch switch` with no target: the Cycle, which picks for you, reading the
/// Accounts it cannot rank without first.
pub fn run_cycle(host: &FakeHost) -> (perch::Result<()>, String) {
    run_switch_with(
        host,
        SwitchArgs {
            target: None,
            no_refresh: false,
        },
    )
}

/// The same Cycle ranking on what is cached, for a test about the ranking
/// rather than about what a Cycle reads before making one.
pub fn run_cycle_on_cache(host: &FakeHost) -> (perch::Result<()>, String) {
    run_switch_with(
        host,
        SwitchArgs {
            target: None,
            no_refresh: true,
        },
    )
}

fn run_switch_with(host: &FakeHost, args: SwitchArgs) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::switch::run(host, args, written)
    })
}

/// It only ends because somebody stopped it, so a fake nobody interrupts leaves
/// this spinning: `with_interrupt_after` says how many rounds the test is
/// about.
pub fn run_watch(host: &FakeHost) -> (perch::Result<()>, String) {
    let (result, printed) = run_watcher(host, WatcherCommand::Run);
    (result.map(|_| ()), printed)
}

/// One round, and the exit code it reports to whatever scheduled it.
pub fn run_watch_once(host: &FakeHost) -> (perch::Result<i32>, String) {
    run_watcher(host, WatcherCommand::Check)
}

/// Through the dispatch rather than into the loop, which is how `servicing.rs`
/// drives the Watcher's other three verbs: an arm wired to the wrong half is
/// the mistake one noun over five verbs makes possible.
fn run_watcher(host: &FakeHost, command: WatcherCommand) -> (perch::Result<i32>, String) {
    ran(host, |host, written| {
        perch::commands::watcher::run(host, command, written)
    })
}

/// The Credential the active Account is watched with: months of life left at
/// the clock these tests run at, so a round is a Refresh of Utilization rather
/// than a Renewal of a token.
pub const ACTIVE: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-active","refreshToken":"sk-ant-ort01-active","expiresAt":1790000000000,"subscriptionType":"pro"}}"#;
pub const ACTIVE_TOKEN: &str = "sk-ant-oat01-active";

/// The same for the Account it would Cycle to.
pub const SPARE: &str = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-spare","refreshToken":"sk-ant-ort01-spare","expiresAt":1790000000000,"subscriptionType":"max"}}"#;
pub const SPARE_TOKEN: &str = "sk-ant-oat01-spare";

/// Two Accounts declared interchangeable, the first one active, and the Group
/// told the watcher may act on it — the machine both `perch watcher run` and
/// `perch watcher check` have something to do on.
pub fn watched() -> FakeHost {
    let host = logged_in_machine().with_login(login_producing(SPARE, SECOND_IDENTITY_FILE));
    // Before the first command, so adoption keeps this Credential rather than
    // the fixture's short-lived one.
    host.set_keychain_item(DEFAULT_SERVICE, LOGIN_NAME, ACTIVE);

    run_add(
        &host,
        AddArgs {
            no_group: true,
            ..AddArgs::default()
        },
    )
    .0
    .expect("the second Account is added");

    a_group_of(&host, "work", &[EMAIL, SECOND_EMAIL]);
    config_set(&host, &["work", "watcher-may-act", "true"])
        .0
        .expect("the Group says the watcher may act");
    host.forget_effects();
    host
}

/// What the usage endpoint answers: both Quota Windows, because a reply leaving
/// one out is one Perch refuses (ADR headroom-is-the-worst-window). The
/// seven-day window sits at nought so the five-hour one is always the fullest,
/// and every figure a trace asserts on is still the one it set.
pub fn usage(used_percent: f64) -> String {
    format!(
        r#"{{"five_hour": {{"utilization": {used_percent}, "resets_at": "2026-08-04T14:30:00Z"}},
            "seven_day": {{"utilization": 0, "resets_at": "2026-08-09T00:00:00Z"}}}}"#
    )
}

pub fn profile_of(email: &str) -> String {
    format!(r#"{{"account": {{"email_address": "{email}"}}}}"#)
}

/// An Account that answers about itself, with its Utilization following a
/// trace: the first figure for the first Refresh, and the last one for every
/// one after the trace runs out.
pub fn answering(host: FakeHost, token: &str, email: &str, trace: &[f64]) -> FakeHost {
    let bodies: Vec<String> = trace.iter().copied().map(usage).collect();
    let replies: Vec<(u16, &str)> = bodies.iter().map(|body| (200, body.as_str())).collect();
    host.with_reply_to(PROFILE_URL, token, 200, &profile_of(email))
        .with_replies_to(USAGE_URL, token, &replies)
}

/// The decision lines: everything printed but what is being watched and that it
/// stopped. Found by the shape a round's line opens with — the stamp, the word
/// it is read by, then the figure. Matching a word the line argues with instead
/// finds nothing and passes every assertion vacuously over an empty list
/// (ADR perch-says-what-it-did).
pub fn decisions(printed: &str) -> Vec<String> {
    printed
        .lines()
        .filter(|line| is_a_decision(line))
        .map(str::to_string)
        .collect()
}

/// Whether a line is a round's. The heartbeat a long hold prints once an hour
/// and the line that says a hold is over share the stamp and the column, and
/// neither is a decision — so the figure is what tells them apart, said as
/// `40% used, …` or as the `unread` that stands in for one.
fn is_a_decision(line: &str) -> bool {
    let Some((_, rest)) = line.split_once("Z  ") else {
        return false;
    };
    rest.contains("% used") || rest.split_whitespace().nth(1) == Some("unread")
}

pub fn run_alias(host: &FakeHost, command: AliasCommand) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::alias::run(host, command, written)
    })
}

/// `perch alias <target> <name>`, taken the other way round: flipping the
/// parameters to match would rewrite every call site in a dozen test files this
/// change has no other business in, and the command line is what
/// ADR a-command-names-its-noun's rule is about.
pub fn set_alias(host: &FakeHost, name: &str, target: &str) -> (perch::Result<()>, String) {
    run_alias(
        host,
        AliasCommand::Set {
            target: target.to_string(),
            name: name.to_string(),
        },
    )
}

pub fn unset_alias(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_alias(
        host,
        AliasCommand::Unset {
            target: target.to_string(),
        },
    )
}

/// `perch add` with a Group named outright, so nothing is asked.
pub fn add_to_group(group: &str) -> AddArgs {
    AddArgs {
        group: Some(group.to_string()),
        ..AddArgs::default()
    }
}

pub fn run_enable(host: &FakeHost, command: EnableCommand) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::enable::run(host, command, written)
    })
}

pub fn disable_account(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_enable(
        host,
        EnableCommand::Disable {
            target: target.to_string(),
        },
    )
}

pub fn enable_account(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_enable(
        host,
        EnableCommand::Enable {
            target: target.to_string(),
        },
    )
}

pub fn run_config(host: &FakeHost, command: ConfigCommand) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::config::run(host, command, written)
    })
}

/// `perch config set <words...>` — the words after `set`, exactly as they would
/// be typed, because which of them names a Group is part of what is under test.
pub fn config_set(host: &FakeHost, words: &[&str]) -> (perch::Result<()>, String) {
    run_config(
        host,
        ConfigCommand::Set {
            words: worded(words),
        },
    )
}

pub fn config_get(host: &FakeHost, words: &[&str]) -> (perch::Result<()>, String) {
    run_config(
        host,
        ConfigCommand::Get {
            words: worded(words),
        },
    )
}

fn worded(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// True where `printed` holds a page row of exactly this key and value. Read as
/// words rather than as a substring, because the column between them is a width
/// no test should have to know.
pub fn row(printed: &str, key: &str, value: &str) -> bool {
    printed
        .lines()
        .any(|line| line.split_whitespace().collect::<Vec<_>>() == [key, value])
}

/// Every Scope a bare `perch config get` named, in the order it printed them.
pub fn scopes_in(printed: &str) -> Vec<String> {
    printed
        .lines()
        .filter_map(|line| line.strip_suffix(':'))
        .map(str::to_string)
        .collect()
}

/// The rows under one Scope's header, where a bare `perch config get` pages
/// every Scope: what a reader looking that Scope up would have in front of them.
pub fn page_of(printed: &str, scope: &str) -> String {
    printed
        .lines()
        .skip_while(|line| *line != format!("{scope}:"))
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// One Quota Window, as full as the test says and with no reset time recorded.
pub fn window(name: &str, used_percent: f64) -> WindowUtilization {
    WindowUtilization {
        window: name.to_string(),
        used_percent,
        resets_at: None,
    }
}

/// The same, carrying when it next resets — what the all-exhausted answer is
/// built out of, and what the soonest-reset Strategy ranks on.
pub fn resetting(name: &str, used_percent: f64, at: DateTime<Utc>) -> WindowUtilization {
    WindowUtilization {
        resets_at: Some(at),
        ..window(name, used_percent)
    }
}

/// Puts figures in the cache for an Account, where a `--refresh` four minutes
/// ago would have left them. Written rather than fetched: the wire refuses a
/// reply missing either Quota Window, while these call sites cache one-window
/// shapes and figures with no reset recorded — states no command run now leaves.
pub fn observed(host: &FakeHost, email: &str, windows: Vec<WindowUtilization>) {
    let observed_at = host.now() - Duration::minutes(4);
    let mut registry = registry_of(host);
    registry
        .account_mut(email)
        .expect("an Account Perch holds")
        .utilization = Some(CachedUtilization {
        observed_at,
        windows,
    });
    save_registry(host, &registry);
}

/// The same, where a `--refresh` a second ago would have left them: a figure
/// inside the Watcher's interval, which a Cycle ranks on rather than reads again
/// (ADR a-choice-reads-what-it-ranks).
pub fn observed_just_now(host: &FakeHost, email: &str, windows: Vec<WindowUtilization>) {
    let observed_at = host.now() - Duration::seconds(1);
    let mut registry = registry_of(host);
    registry
        .account_mut(email)
        .expect("an Account Perch holds")
        .utilization = Some(CachedUtilization {
        observed_at,
        windows,
    });
    save_registry(host, &registry);
}

/// Marks an Account as one whose Credential can no longer be used and cannot be
/// recovered — the state a rejected Renewal leaves behind. `quarantining.rs`
/// proves `status --refresh` creates every reason from the wire; this serves
/// the tests about what Perch does with the state afterwards.
pub fn quarantine(host: &FakeHost, email: &str) {
    quarantine_for(host, email, Quarantine::RenewalRejected);
}

/// The same, for the tests that are about one particular reason.
pub fn quarantine_for(host: &FakeHost, email: &str, why: Quarantine) {
    let mut registry = registry_of(host);
    assert!(
        registry.quarantine(email, why),
        "{email} is an Account Perch holds and was not already Quarantined"
    );
    save_registry(host, &registry);
}

pub fn run_export(host: &FakeHost, path: &str) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::export::run(host, &std::path::PathBuf::from(path), written)
    })
}

pub fn run_import(host: &FakeHost, path: &str) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::import::run(host, &std::path::PathBuf::from(path), written)
    })
}

pub fn run_purge(host: &FakeHost) -> (perch::Result<()>, String) {
    run_purge_with(host, false)
}

/// The same, for the tests that are about the flag a script purges with.
pub fn run_purge_with(host: &FakeHost, yes: bool) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::purge::run(host, yes, written)
    })
}

pub fn run_relogin(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    let args = ReloginArgs {
        target: target.to_string(),
    };
    ran(host, |host, written| {
        perch::commands::relogin::run(host, args, written)
    })
}

pub fn run_remove(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_remove_with(
        host,
        RemoveArgs {
            target: target.to_string(),
            yes: false,
        },
    )
}

/// The same, for the tests that are about the flag a script removes with.
pub fn run_remove_with(host: &FakeHost, args: RemoveArgs) -> (perch::Result<()>, String) {
    ran(host, |host, written| {
        perch::commands::remove::run(host, args, written)
    })
}

/// Why an Account is Quarantined, as the Registry records it.
pub fn quarantine_of(host: &FakeHost, email: &str) -> Option<Quarantine> {
    registry_of(host)
        .account(email)
        .expect("an Account Perch holds")
        .quarantine
}
