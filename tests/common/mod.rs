//! Fixtures for the behaviour tests: a machine with a Claude Code login on it.

// Each test binary gets its own copy of this module and uses the part of it
// that it needs, so unused fixtures here are the normal case rather than rot.
#![allow(dead_code)]

use std::path::Path;

use perch::commands::add::AddArgs;
use perch::commands::status::StatusArgs;
use perch::host::{Execution, FakeHost};
use perch::probe;

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

/// A machine with Claude Code installed but nobody logged in.
pub fn machine_with_claude_code() -> FakeHost {
    FakeHost::new().with_exec(
        "claude",
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

/// A login that completes, leaving behind exactly what Claude Code would in
/// the config directory it was pointed at: the Credential in that directory's
/// own keychain namespace, and the Identity in its `.claude.json`.
pub fn login_producing(
    credential: &'static str,
    identity_file: &'static str,
) -> impl Fn(&FakeHost, &Path) -> i32 {
    move |host, dir| {
        host.set_keychain_item(&probe::service_name_for(dir, false), LOGIN_NAME, credential);
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
    let mut written = Vec::new();
    let result = perch::commands::status::run(host, StatusArgs { json }, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// Runs `perch add`, returning what it printed alongside how it ended.
pub fn run_add(host: &FakeHost, args: AddArgs) -> (perch::Result<()>, String) {
    let mut written = Vec::new();
    let result = perch::commands::add::run(host, args, &mut written);
    (result, String::from_utf8(written).expect("output is UTF-8"))
}

/// `perch add` with a Group named outright, so nothing is asked.
pub fn add_to_group(group: &str) -> AddArgs {
    AddArgs {
        group: Some(group.to_string()),
        ..AddArgs::default()
    }
}
