//! Fixtures for the behaviour tests: a machine with a Claude Code login on it.

// Each test binary gets its own copy of this module and uses the part of it
// that it needs, so unused fixtures here are the normal case rather than rot.
#![allow(dead_code)]

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use perch::commands::add::AddArgs;
use perch::commands::alias::AliasCommand;
use perch::commands::config::ConfigCommand;
use perch::commands::enable::EnableCommand;
use perch::commands::group::GroupCommand;
use perch::commands::list::ListArgs;
use perch::commands::status::StatusArgs;
use perch::commands::switch::SwitchArgs;
use perch::credentials;
use perch::host::{Execution, FakeHost, Host, Platform};
use perch::probe;
use perch::registry::{CachedUtilization, WindowUtilization};

pub const CLAUDE_VERSION: &str = "2.1.221";
pub const LOGIN_NAME: &str = "someone";
pub const EMAIL: &str = "someone@example.com";
pub const DEFAULT_SERVICE: &str = "Claude Code-credentials";
pub const REGISTRY_PATH: &str = "/Users/someone/.perch/registry.json";
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
/// its Credential in the file that is the store there (ADR 0020).
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

/// Runs `perch status` against a fake machine, returning what it printed
/// alongside how it ended.
pub fn run_status(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_status_with(
        host,
        StatusArgs {
            json,
            ..StatusArgs::default()
        },
    )
}

/// `perch status --group`: the same cached view, narrowed to the Group the
/// active Account is in.
pub fn run_status_group(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_status_with(
        host,
        StatusArgs {
            json,
            group: true,
            ..StatusArgs::default()
        },
    )
}

/// `perch status --refresh`: the one command that fetches (ADR 0015).
pub fn run_status_refresh(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_status_with(
        host,
        StatusArgs {
            json,
            refresh: true,
            ..StatusArgs::default()
        },
    )
}

/// `perch status --group --refresh`: fetch for every Account you could land on.
pub fn run_status_group_refresh(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    run_status_with(
        host,
        StatusArgs {
            json,
            group: true,
            refresh: true,
        },
    )
}

/// `perch status` with whatever combination of flags the test is about.
pub fn run_status_with(host: &FakeHost, args: StatusArgs) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::status::run(host, args, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// Runs `perch list`, returning what it printed alongside how it ended.
pub fn run_list(host: &FakeHost, json: bool) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::list::run(host, ListArgs { json }, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// A machine holding two Accounts, neither in a Group and neither named: the
/// ordinary starting state once a second Account has been added (ADR 0017).
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
    declare_group(&host, "work");
    for email in [EMAIL, SECOND_EMAIL, THIRD_EMAIL] {
        move_to_group(&host, email, "work")
            .0
            .expect("the Account joins the Group");
    }
    host
}

/// Runs `perch add`, returning what it printed alongside how it ended.
pub fn run_add(host: &FakeHost, args: AddArgs) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::add::run(host, args, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// Where an Account's Profile keeps its things, derived the way every command
/// derives it now that nothing records it (ADR 0020).
pub fn store_of(host: &FakeHost, email: &str) -> probe::Store {
    let dir = perch::registry::profile_dir_for(host, email).expect("home is known");
    probe::store_for_profile(host, &dir).expect("USER is set")
}

/// The Credential a Profile holds, from whichever of its two Credential Stores
/// holds one — which is the only question a test about a stored Credential
/// should be asking.
pub fn credential_of(host: &FakeHost, email: &str) -> Option<String> {
    perch::credentials::read(host, &store_of(host, email))
        .expect("the store could be consulted")
        .map(|held| held.credential)
}

/// The registry as it would be read back by the next command Perch runs.
pub fn registry_of(host: &FakeHost) -> perch::registry::Registry {
    perch::registry::load(host)
        .unwrap()
        .expect("a registry is written")
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

/// `perch group move <target> <group>`.
pub fn move_to_group(host: &FakeHost, target: &str, group: &str) -> (perch::Result<()>, String) {
    run_group(
        host,
        GroupCommand::Move {
            target: target.to_string(),
            group: group.to_string(),
        },
    )
}

/// Runs `perch group`, returning what it printed alongside how it ended.
pub fn run_group(host: &FakeHost, command: GroupCommand) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::group::run(host, command, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// Runs `perch switch <target>`, returning what it printed alongside how it
/// ended.
pub fn run_switch(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_switch_with(
        host,
        SwitchArgs {
            target: Some(target.to_string()),
        },
    )
}

/// `perch switch` with no target: the Cycle, which picks for you.
pub fn run_cycle(host: &FakeHost) -> (perch::Result<()>, String) {
    run_switch_with(host, SwitchArgs { target: None })
}

fn run_switch_with(host: &FakeHost, args: SwitchArgs) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::switch::run(host, args, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// Runs `perch alias`, returning what it printed alongside how it ended.
pub fn run_alias(host: &FakeHost, command: AliasCommand) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::alias::run(host, command, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// `perch alias <name> <target>`.
pub fn set_alias(host: &FakeHost, name: &str, target: &str) -> (perch::Result<()>, String) {
    run_alias(
        host,
        AliasCommand::Set {
            name: name.to_string(),
            target: target.to_string(),
        },
    )
}

/// `perch alias <name> --unset`.
pub fn unset_alias(host: &FakeHost, name: &str) -> (perch::Result<()>, String) {
    run_alias(
        host,
        AliasCommand::Unset {
            name: name.to_string(),
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

/// Runs `perch enable` or `perch disable`, returning what it printed alongside
/// how it ended.
pub fn run_enable(host: &FakeHost, command: EnableCommand) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::enable::run(host, command, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// `perch disable <target>`.
pub fn disable_account(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_enable(
        host,
        EnableCommand::Disable {
            target: target.to_string(),
        },
    )
}

/// `perch enable <target>`.
pub fn enable_account(host: &FakeHost, target: &str) -> (perch::Result<()>, String) {
    run_enable(
        host,
        EnableCommand::Enable {
            target: target.to_string(),
        },
    )
}

/// Runs `perch config`, returning what it printed alongside how it ended.
pub fn run_config(host: &FakeHost, command: ConfigCommand) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::config::run(host, command, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
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

/// `perch config get [<words...>]`.
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
/// ago would have left them.
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
    perch::registry::save(host, &registry).expect("the registry is written");
}

/// Marks an Account as one whose Credential can no longer be used and cannot be
/// recovered. Quarantine is #13; this is the state it leaves behind.
pub fn quarantine(host: &FakeHost, email: &str) {
    let mut registry = registry_of(host);
    registry.account_mut(email).expect("an Account").quarantined = true;
    perch::registry::save(host, &registry).expect("the registry is written");
}
